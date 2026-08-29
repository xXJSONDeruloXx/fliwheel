//! Small native library UI for the decrypted clickwheel-game runner.
//!
//! The emulator itself remains a separate process for each launch. This keeps
//! the game window's lifetime and audio device isolated while making the
//! common "pick a game, play it, come back" workflow straightforward.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use minifb::{Key, KeyRepeat, ScaleMode, Window, WindowOptions};

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

const BG: u32 = 0x10151f;
const PANEL: u32 = 0x182233;
const PANEL_ALT: u32 = 0x202e42;
const HEADER: u32 = 0x263e5e;
const ACCENT: u32 = 0x46b5ff;
const ACCENT_DARK: u32 = 0x214d77;
const TEXT: u32 = 0xeaf3ff;
const MUTED: u32 = 0x9eb1c7;
const GOOD: u32 = 0x7ee787;
const WARN: u32 = 0xffd166;

#[derive(Debug, Clone)]
struct GameEntry {
    id: String,
    title: String,
    bundle_dir: PathBuf,
    source_root: PathBuf,
}

#[derive(Debug)]
struct Library {
    roots: Vec<PathBuf>,
    ignored_roots: Vec<PathBuf>,
    games: Vec<GameEntry>,
    selected: usize,
    message: String,
}

impl Library {
    fn discover(initial_root: Option<PathBuf>) -> Self {
        let (mut roots, ignored_roots) = load_saved_roots();
        if let Some(root) = initial_root {
            add_unique_path(&mut roots, root);
        }
        if let Some(root) = env::var_os("FLIWHEEL_GAMES_ROOT") {
            add_unique_path(&mut roots, PathBuf::from(root));
        }
        for root in default_roots() {
            add_unique_path(&mut roots, root);
        }
        roots.retain(|root| !ignored_roots.iter().any(|ignored| ignored == root));

        let mut library = Self {
            roots,
            ignored_roots,
            games: Vec::new(),
            selected: 0,
            message: String::new(),
        };
        library.refresh();
        library
    }

    fn refresh(&mut self) {
        let previous = self
            .games
            .get(self.selected)
            .map(|game| game.bundle_dir.clone());
        let mut games = Vec::new();
        for root in &self.roots {
            scan_root(root, &mut games);
        }
        games.sort_by(|left, right| {
            left.id.cmp(&right.id).then_with(|| {
                left.title
                    .to_ascii_lowercase()
                    .cmp(&right.title.to_ascii_lowercase())
            })
        });
        games.dedup_by(|left, right| left.bundle_dir == right.bundle_dir);
        self.games = games;
        self.selected = previous
            .and_then(|path| self.games.iter().position(|game| game.bundle_dir == path))
            .unwrap_or(0)
            .min(self.games.len().saturating_sub(1));
    }

    fn move_selection(&mut self, delta: isize) {
        if self.games.is_empty() {
            return;
        }
        let len = self.games.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
    }

    fn add_root(&mut self, root: PathBuf) {
        let path = normalize_path(root);
        if !path.is_dir() {
            self.message = format!("Folder not found: {}", path.display());
            return;
        }
        if self.roots.iter().any(|existing| existing == &path) {
            self.message = "That folder is already in the library".to_string();
            return;
        }
        self.ignored_roots.retain(|ignored| ignored != &path);
        self.roots.push(path.clone());
        save_roots(&self.roots, &self.ignored_roots);
        self.refresh();
        self.message = format!("Added source: {}", path.display());
    }

    fn forget_selected_source(&mut self) {
        let Some(game) = self.games.get(self.selected) else {
            self.message = "There is no selected game".to_string();
            return;
        };
        let source = game.source_root.clone();
        self.roots.retain(|root| root != &source);
        if !self.ignored_roots.iter().any(|ignored| ignored == &source) {
            self.ignored_roots.push(source.clone());
        }
        save_roots(&self.roots, &self.ignored_roots);
        self.refresh();
        self.message = format!("Forgot source; files kept: {}", source.display());
    }

    fn launch_selected(&mut self) {
        let Some(game) = self.games.get(self.selected).cloned() else {
            self.message = "Add a Games_RO folder first".to_string();
            return;
        };
        let executable = match env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                self.message = format!("Could not locate the runner: {error}");
                return;
            }
        };
        self.message = format!("Launching {}...", game.title);
        match Command::new(executable).arg(&game.bundle_dir).status() {
            Ok(status) if status.success() => {
                self.message = format!("Returned from {}", game.title);
            }
            Ok(status) => {
                self.message = format!("{} exited with {}", game.title, status);
            }
            Err(error) => {
                self.message = format!("Could not launch {}: {error}", game.title);
            }
        }
        self.refresh();
    }
}

pub fn run_library_ui(initial_root: Option<PathBuf>) -> Result<(), String> {
    let mut library = Library::discover(initial_root);
    let mut window = Window::new(
        "FLIWHEEL | Library",
        WIDTH,
        HEIGHT,
        WindowOptions {
            scale_mode: ScaleMode::AspectRatioStretch,
            resize: true,
            ..WindowOptions::default()
        },
    )
    .map_err(|error| format!("could not create library window: {error}"))?;
    window.limit_update_rate(Some(std::time::Duration::from_micros(16_600)));

    let mut buffer = vec![BG; WIDTH * HEIGHT];
    'ui: while window.is_open() {
        for key in window.get_keys_pressed(KeyRepeat::Yes) {
            match key {
                Key::Up => library.move_selection(-1),
                Key::Down => library.move_selection(1),
                Key::Enter | Key::Space => library.launch_selected(),
                Key::A | Key::I => match choose_folder() {
                    Some(path) => library.add_root(path),
                    None => library.message = "Add canceled".to_string(),
                },
                Key::R => {
                    library.refresh();
                    library.message = "Library refreshed".to_string();
                }
                Key::Delete | Key::Backspace => library.forget_selected_source(),
                Key::Escape | Key::Q => break 'ui,
                _ => {}
            }
        }

        draw_library(&library, &mut buffer);
        window
            .update_with_buffer(&buffer, WIDTH, HEIGHT)
            .map_err(|error| format!("could not update library window: {error}"))?;
    }
    Ok(())
}

fn draw_library(library: &Library, buffer: &mut [u32]) {
    buffer.fill(BG);
    fill_rect(buffer, 0, 0, WIDTH, 62, HEADER);
    draw_text(buffer, 24, 12, "FLIWHEEL LIBRARY", 3, TEXT);
    draw_text(
        buffer,
        25,
        43,
        &format!(
            "{} GAMES  |  {} SOURCES",
            library.games.len(),
            library.roots.len()
        ),
        1,
        MUTED,
    );

    fill_rect(buffer, 18, 78, 360, 354, PANEL);
    fill_rect(buffer, 394, 78, 228, 354, PANEL);
    draw_text(buffer, 30, 91, "YOUR GAMES", 2, TEXT);
    draw_text(buffer, 407, 91, "SELECTED", 2, TEXT);

    const ROW_Y: usize = 121;
    const ROW_H: usize = 24;
    let visible = (HEIGHT.saturating_sub(ROW_Y + 25)) / ROW_H;
    if library.games.is_empty() {
        fill_rect(buffer, 30, ROW_Y, 336, 54, PANEL_ALT);
        draw_text(buffer, 44, ROW_Y + 11, "NO GAMES FOUND", 2, WARN);
        draw_text(buffer, 44, ROW_Y + 32, "PRESS A TO ADD A FOLDER", 1, MUTED);
    } else {
        let first = library.selected.saturating_sub(visible.saturating_sub(1));
        for (visible_index, game_index) in (first..library.games.len()).take(visible).enumerate() {
            let y = ROW_Y + visible_index * ROW_H;
            let selected = game_index == library.selected;
            if selected {
                fill_rect(buffer, 28, y - 2, 340, ROW_H - 1, ACCENT_DARK);
                fill_rect(buffer, 28, y - 2, 4, ROW_H - 1, ACCENT);
            }
            let game = &library.games[game_index];
            draw_text(
                buffer,
                40,
                y + 3,
                &format!("{}  {}", game.id, game.title),
                2,
                if selected { TEXT } else { MUTED },
            );
        }
    }

    if let Some(game) = library.games.get(library.selected) {
        draw_text(buffer, 408, 124, &game.title, 2, TEXT);
        draw_text(buffer, 408, 146, &format!("ID {}", game.id), 2, ACCENT);
        draw_text(buffer, 408, 180, "BUNDLE", 1, MUTED);
        draw_text(
            buffer,
            408,
            197,
            &game
                .bundle_dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| game.bundle_dir.display().to_string()),
            2,
            TEXT,
        );
        draw_text(buffer, 408, 235, "SOURCE", 1, MUTED);
        draw_text(
            buffer,
            408,
            252,
            &game
                .source_root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| game.source_root.display().to_string()),
            1,
            MUTED,
        );
        draw_text(buffer, 408, 290, "ENTER / SPACE  PLAY", 1, GOOD);
    } else {
        draw_text(buffer, 408, 128, "ADD A GAMES_RO", 1, MUTED);
        draw_text(buffer, 408, 146, "FOLDER TO BEGIN", 1, MUTED);
    }

    fill_rect(buffer, 18, 445, 604, 23, PANEL_ALT);
    draw_text(
        buffer,
        28,
        451,
        "UP/DOWN SELECT   A ADD   R REFRESH   DEL FORGET SOURCE   ESC QUIT",
        1,
        MUTED,
    );
    if !library.message.is_empty() {
        draw_text(buffer, 28, 462, &library.message, 1, WARN);
    }
}

fn scan_root(root: &Path, games: &mut Vec<GameEntry>) {
    let root = normalize_path(root.to_path_buf());
    if !root.is_dir() {
        return;
    }
    if is_bundle_dir(&root) {
        add_game(&root, &root, games);
        return;
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && is_bundle_dir(&path) {
            add_game(&path, &root, games);
        }
    }
}

fn add_game(bundle_dir: &Path, source_root: &Path, games: &mut Vec<GameEntry>) {
    let id = bundle_dir
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "GAME".to_string());
    let title =
        manifest_string(&bundle_dir.join("Manifest.plist"), "Name").unwrap_or_else(|| id.clone());
    games.push(GameEntry {
        id,
        title,
        bundle_dir: bundle_dir.to_path_buf(),
        source_root: source_root.to_path_buf(),
    });
}

fn is_bundle_dir(path: &Path) -> bool {
    path.join("Manifest.plist").is_file()
}

fn manifest_string(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let marker = format!("<key>{key}</key>");
    let value = text.split_once(&marker)?.1;
    let start = value.find("<string>")? + "<string>".len();
    let end = value[start..].find("</string>")? + start;
    Some(xml_unescape(&value[start..end]))
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn load_saved_roots() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Some(path) = config_path() else {
        return (Vec::new(), Vec::new());
    };
    let Ok(text) = fs::read_to_string(path) else {
        return (Vec::new(), Vec::new());
    };
    let mut roots = Vec::new();
    let mut ignored_roots = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let (ignored, raw_path) = line
            .strip_prefix('!')
            .map(|path| (true, path))
            .unwrap_or((false, line));
        let path = normalize_path(PathBuf::from(raw_path));
        if ignored {
            if path.is_dir() {
                add_unique_path(&mut ignored_roots, path);
            }
        } else if path.is_dir() {
            add_unique_path(&mut roots, path);
        }
    }
    (roots, ignored_roots)
}

fn save_roots(roots: &[PathBuf], ignored_roots: &[PathBuf]) {
    let Some(path) = config_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut lines = roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>();
    lines.extend(
        ignored_roots
            .iter()
            .map(|root| format!("!{}", root.display())),
    );
    let _ = fs::write(path, format!("{}\n", lines.join("\n")));
}

fn config_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("FLIWHEEL_CONFIG_DIR") {
        return Some(PathBuf::from(path).join("library.txt"));
    }
    let home_dir = env::var_os("HOME").map(PathBuf::from)?;
    let base = if cfg!(target_os = "macos") {
        home_dir.join("Library/Application Support/fliwheel")
    } else if let Some(config) = env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config).join("fliwheel")
    } else {
        home_dir.join(".config/fliwheel")
    };
    Some(base.join("library.txt"))
}

fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(current) = env::current_dir() {
        roots.push(current.join("Games_RO"));
    }
    if let Some(home_dir) = env::var_os("HOME").map(PathBuf::from) {
        roots.push(home_dir.join("Downloads/16-ipod-games/Games_RO"));
    }
    roots.push(PathBuf::from(
        "/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO",
    ));
    roots
}

fn normalize_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

fn add_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    let path = normalize_path(path);
    if path.is_dir() && !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn choose_folder() -> Option<PathBuf> {
    let output = Command::new("osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"Choose a Games_RO folder or game bundle\")",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn fill_rect(buffer: &mut [u32], x: usize, y: usize, width: usize, height: usize, color: u32) {
    let x_end = x.saturating_add(width).min(WIDTH);
    let y_end = y.saturating_add(height).min(HEIGHT);
    for row in y.min(HEIGHT)..y_end {
        for column in x.min(WIDTH)..x_end {
            buffer[row * WIDTH + column] = color;
        }
    }
}

fn draw_text(buffer: &mut [u32], x: usize, y: usize, text: &str, scale: usize, color: u32) {
    let mut cursor = x;
    for character in text.chars() {
        if character == '\n' {
            continue;
        }
        let rows = glyph(character.to_ascii_uppercase());
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) != 0 {
                    fill_rect(
                        buffer,
                        cursor + column * scale,
                        y + row * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
        cursor += 4 * scale;
    }
}

/// A compact 3x5 bitmap font keeps the library UI self-contained and avoids
/// adding a separate font asset to the emulator repository.
fn glyph(character: char) -> [u8; 5] {
    match character {
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b010, 0b001],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b110, 0b001, 0b010, 0b100, 0b111],
        '3' => [0b110, 0b001, 0b010, 0b001, 0b110],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b110, 0b001, 0b110],
        '6' => [0b011, 0b100, 0b111, 0b101, 0b010],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b010, 0b101, 0b010, 0b101, 0b010],
        '9' => [0b010, 0b101, 0b111, 0b001, 0b110],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '_' => [0b000, 0b000, 0b000, 0b000, 0b111],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '\'' => [0b010, 0b010, 0b000, 0b000, 0b000],
        _ => [0; 5],
    }
}

#[cfg(test)]
mod tests {
    use super::{glyph, manifest_string, xml_unescape};
    use std::fs;

    #[test]
    fn bitmap_font_has_readable_game_id_glyphs() {
        assert_ne!(glyph('A'), [0; 5]);
        assert_ne!(glyph('6'), [0; 5]);
        assert_eq!(glyph('?'), [0; 5]);
    }

    #[test]
    fn manifest_names_and_entities_are_read() {
        let path = std::env::temp_dir().join(format!(
            "fliwheel-library-test-{}-{}.plist",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(
            &path,
            "<key>Name</key><string>Texas Hold&apos;em &amp; Friends</string>",
        )
        .expect("write plist fixture");
        assert_eq!(
            manifest_string(&path, "Name").as_deref(),
            Some("Texas Hold'em & Friends")
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn xml_unescape_handles_common_plist_entities() {
        assert_eq!(
            xml_unescape("&lt;A&gt; &quot;B&quot; &apos;C&apos; &amp; D"),
            "<A> \"B\" 'C' & D"
        );
    }
}

#[cfg(not(test))]
#[allow(dead_code)]
fn main() {
    if let Err(error) = run_library_ui(None) {
        eprintln!("fliwheel library: {error}");
        std::process::exit(1);
    }
}
