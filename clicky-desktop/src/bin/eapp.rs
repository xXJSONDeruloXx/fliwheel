#[macro_use]
extern crate log;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc as chan;

use clicky_core::gui::{ButtonCallback, RenderCallback, ScrollCallback, TakeControls};
use clicky_core::sys::eapp::{Eapp, EappBinds, EappKey};
use minifb::{Key, ScaleMode, Window, WindowOptions};
use structopt::StructOpt;

pub type DynResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(StructOpt, Debug)]
#[structopt(name = "clicky-eapp")]
#[structopt(about = "Run an iPod clickwheel game executable via the experimental eapp runner.")]
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

fn run_minifb_ui(
    title: String,
    mut update_fb: RenderCallback,
    controls: impl Into<MinifbControls>,
    kill_rx: chan::Receiver<()>,
) {
    let mut controls = controls.into();

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
        .filter(Some("clicky"), log::LevelFilter::Trace)
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

    run_minifb_ui(title, update_fb, controls, kill_rx);

    Ok(())
}
