#[macro_use]
extern crate log;

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc as chan;

use fliwheel_core::gui::{ButtonCallback, RenderCallback, ScrollCallback, TakeControls};
use fliwheel_core::sys::eapp::{Eapp, EappAudioEventQueue, EappBinds, EappKey};
use minifb::{Key, ScaleMode, Window, WindowOptions};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use structopt::StructOpt;

pub type DynResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(StructOpt, Debug)]
#[structopt(name = "fliwheel-eapp")]
#[structopt(about = "Run a decrypted iPod clickwheel game via the EAPP runner.")]
struct Args {
    /// Path to a Games_RO/<id> bundle directory.
    #[structopt(parse(from_os_str))]
    bundle_dir: PathBuf,

    /// Run a fixed number of CPU cycles and then exit.
    #[structopt(long)]
    cycles: Option<usize>,

    /// Disable the minifb UI and run headless.
    #[structopt(long)]
    headless: bool,
}

struct MinifbControls {
    keymap: HashMap<Key, ButtonCallback>,
    on_scroll: Option<ScrollCallback>,
}

fn eapp_key_to_minifb(key: EappKey) -> Key {
    match key {
        EappKey::Up => Key::Up,
        EappKey::Down => Key::Down,
        EappKey::Left => Key::Left,
        EappKey::Right => Key::Right,
        EappKey::Action => Key::Enter,
        EappKey::Menu => Key::M,
    }
}

impl From<EappBinds> for MinifbControls {
    fn from(binds: EappBinds) -> MinifbControls {
        let EappBinds { keys, wheel } = binds;
        MinifbControls {
            keymap: keys
                .into_iter()
                .map(|(key, callback)| (eapp_key_to_minifb(key), callback))
                .collect(),
            on_scroll: wheel,
        }
    }
}

struct DesktopAudio {
    _stream: OutputStream,
    handle: OutputStreamHandle,
}

impl DesktopAudio {
    fn new() -> Option<Self> {
        if std::env::var_os("EAPP_AUDIO_DISABLE").is_some() {
            info!(target: "EAPP_AUDIO", "desktop audio disabled by EAPP_AUDIO_DISABLE");
            return None;
        }
        match OutputStream::try_default() {
            Ok((_stream, handle)) => Some(Self { _stream, handle }),
            Err(err) => {
                warn!(target: "EAPP_AUDIO", "no desktop audio output: {}", err);
                None
            }
        }
    }

    fn pump(&self, events: &EappAudioEventQueue) {
        let pending = match events.lock() {
            Ok(mut queue) => queue.drain(..).collect::<Vec<_>>(),
            Err(_) => return,
        };
        for event in pending {
            let Some(path) = event.host_path else {
                warn!(
                    target: "EAPP_AUDIO",
                    "unmapped sound event frame={} type={} index={}",
                    event.frame,
                    event.resource_type,
                    event.resource_index
                );
                continue;
            };
            if !is_supported_sound_path(&path) {
                warn!(
                    target: "EAPP_AUDIO",
                    "unsupported sound asset frame={} type={} index={} path={}",
                    event.frame,
                    event.resource_type,
                    event.resource_index,
                    path.display()
                );
                continue;
            }
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(err) => {
                    warn!(
                        target: "EAPP_AUDIO",
                        "could not open sound frame={} path={}: {}",
                        event.frame,
                        path.display(),
                        err
                    );
                    continue;
                }
            };
            let source = match Decoder::new(BufReader::new(file)) {
                Ok(source) => source,
                Err(err) => {
                    warn!(
                        target: "EAPP_AUDIO",
                        "could not decode sound frame={} path={}: {}",
                        event.frame,
                        path.display(),
                        err
                    );
                    continue;
                }
            };
            let sink = match Sink::try_new(&self.handle) {
                Ok(sink) => sink,
                Err(err) => {
                    warn!(target: "EAPP_AUDIO", "could not create sound sink: {}", err);
                    continue;
                }
            };
            sink.append(source);
            sink.detach();
            info!(
                target: "EAPP_AUDIO",
                "played sound frame={} type={} index={} path={}",
                event.frame,
                event.resource_type,
                event.resource_index,
                path.display()
            );
        }
    }
}

fn is_supported_sound_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "wav" | "mp3" | "flac" | "ogg" | "aac" | "mp4" | "m4a" | "m4b" | "m4p" | "m4r"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_supported_sound_path;
    use std::path::Path;

    #[test]
    fn recognizes_ipod_aac_asset_extensions() {
        for path in ["click.wav", "CLICK.M4A", "music.m4b", "stream.aac", "clip.mp4"] {
            assert!(is_supported_sound_path(Path::new(path)), "{}", path);
        }
        assert!(!is_supported_sound_path(Path::new("metadata.bin")));
    }
}

fn run_minifb_ui(
    title: String,
    mut update_fb: RenderCallback,
    controls: impl Into<MinifbControls>,
    audio_events: EappAudioEventQueue,
    kill_rx: chan::Receiver<()>,
) {
    let mut controls = controls.into();
    let audio = DesktopAudio::new();

    let mut window = Window::new(
        &title,
        320,
        240,
        WindowOptions {
            scale: minifb::Scale::X2,
            scale_mode: ScaleMode::AspectRatioStretch,
            resize: true,
            ..WindowOptions::default()
        },
    )
    .expect("could not create minifb window");

    window.limit_update_rate(Some(std::time::Duration::from_micros(16_600)));

    let mut buffer: Vec<u32> = vec![0; 320 * 240];
    let mut emu_buffer = Vec::new();

    'ui: while window.is_open() && kill_rx.try_recv().is_err() {
        if let Some(audio) = audio.as_ref() {
            audio.pump(&audio_events);
        }

        for key in window.get_keys_pressed(minifb::KeyRepeat::Yes) {
            if key == Key::Escape {
                break 'ui;
            }
            if let Some(callback) = controls.keymap.get_mut(&key) {
                callback(true);
            }
        }

        for key in window.get_keys_released() {
            if let Some(callback) = controls.keymap.get_mut(&key) {
                callback(false);
            }
        }

        if let Some(scroll) = window.get_scroll_wheel() {
            if let Some(callback) = controls.on_scroll.as_mut() {
                callback(scroll);
            }
        }

        let (width, _height) = update_fb(&mut emu_buffer);
        let new_buf = emu_buffer
            .chunks_exact(width)
            .take(240)
            .flat_map(|row| row.iter().take(320))
            .copied();
        buffer.splice(.., new_buf);

        window
            .update_with_buffer(&buffer, 320, 240)
            .expect("could not update minifb window");
    }
}

fn main() -> DynResult<()> {
    pretty_env_logger::formatted_builder()
        .filter(None, log::LevelFilter::Error)
        .filter(Some("fliwheel"), log::LevelFilter::Trace)
        .filter(Some("EAPP_IMPORT"), log::LevelFilter::Info)
        .filter(Some("EAPP_GL"), log::LevelFilter::Info)
        .filter(Some("EAPP"), log::LevelFilter::Info)
        .filter(Some("armv4t_emu"), log::LevelFilter::Warn)
        .parse_filters(&std::env::var("RUST_LOG").unwrap_or_default())
        .init();

    let args = Args::from_args();
    let mut system = Eapp::from_bundle_dir(&args.bundle_dir)?;
    if let Ok(spec) = std::env::var("EAPP_GL_TRACE") {
        if let Some((s, e)) = spec.split_once('-') {
            if let (Ok(start), Ok(end)) = (s.parse(), e.parse()) {
                system.set_gl_trace_window(start, end);
                info!(target: "EAPP", "GL trace window enabled for frames {}..={}", start, end);
            }
        }
    }
    let title = format!("{} [eapp]", system.title());

    let capture_path = std::env::var_os("EAPP_GL_CAPTURE_JSON").map(PathBuf::from);
    if let Some(_) = capture_path.as_ref() {
        let (start, end) = std::env::var("EAPP_GL_CAPTURE_FRAMES")
            .ok()
            .and_then(|spec| {
                spec.split_once('-')
                    .map(|(s, e)| (s.to_string(), e.to_string()))
            })
            .and_then(|(s, e)| Some((s.parse::<u64>().ok()?, e.parse::<u64>().ok()?)))
            .unwrap_or((0, 60));
        let stack_len = std::env::var("EAPP_GL_CAPTURE_STACK_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0x80);
        let pointer_len = std::env::var("EAPP_GL_CAPTURE_POINTER_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0x80);
        system.enable_gl_capture(start, end, stack_len, pointer_len);
        info!(target: "EAPP", "GL capture enabled for frames {}..={} -> {}", start, end, capture_path.as_ref().unwrap().display());
    }

    if args.headless {
        let result = match args.cycles {
            Some(cycles) => system.run_cycles(cycles),
            None => system.run(),
        };
        system.log_top_imports(25);
        system.drain_watch_log();
        system.log_dma_stats();
        if std::env::var("EAPP_RAMSCAN").is_ok() {
            system.scan_for_framebuffer();
        }
        if std::env::var("EAPP_STRING_SCAN").is_ok() {
            system.scan_for_strings();
        }
        if let Some(path) = capture_path {
            system
                .write_gl_trace_fixture(&path)
                .map_err(|err| format!("failed to write GL capture {}: {}", path.display(), err))?;
        }
        system.dump_string_trace_totals();
        if let Err(err) = result {
            return Err(format!("fatal eapp error: {:#010x?}", err).into());
        }
        return Ok(());
    }

    let update_fb = system.render_callback();
    let audio_events = system.audio_event_queue();
    let controls = system
        .take_controls()
        .ok_or_else(|| "could not take eapp controls".to_string())?;
    let (kill_tx, kill_rx) = chan::channel();

    let cycles = args.cycles;
    std::thread::spawn(move || {
        let result = match cycles {
            Some(cycles) => system.run_cycles(cycles),
            None => system.run(),
        };
        system.dump_string_trace_totals();
        if let Err(err) = result {
            error!("fatal eapp error: {:#010x?}", err);
        }
        let _ = kill_tx.send(());
    });

    run_minifb_ui(title, update_fb, controls, audio_events, kill_rx);

    Ok(())
}
