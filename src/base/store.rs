//! Paths, logging and safe JSON storage.
//!
//! Everything the app keeps lives in small JSON files. Writes go to a temp file
//! that is then swapped into place, so a crash or power cut can never leave a
//! half-written file; the previous good copy is kept as `<name>.bak`, and a file
//! that turns out to be unreadable is set aside and the backup used instead.

use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(dead_code)]
pub struct Paths {
    pub base: PathBuf,
    pub library: PathBuf,
    pub covers: PathBuf,
    pub data: PathBuf,
    pub static_dir: PathBuf,
    pub playlists: PathBuf,
    pub stats: PathBuf,
    pub likes: PathBuf,
    pub history: PathBuf,
    pub settings: PathBuf,
    pub sources: PathBuf,
    pub loudness: PathBuf,
    pub meta: PathBuf,
    pub player_state: PathBuf,
    pub playlist_covers: PathBuf,
    pub lyrics: PathBuf,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

/// Bumped whenever a file that feeds the library list (play counts, edited names/albums, loudness)
/// is written, so the cached list knows it is out of date.
pub static DATA_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Bumped when metadata.json (names, albums) is written, so the app notices edits without re-reading the whole library.
pub static META_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Where the library lives when `MUSIC_HOME` isn't set: the current directory if it already holds
/// a library (running from a checkout), otherwise `%LOCALAPPDATA%\Vinyl` so an installed copy
/// keeps its music and settings outside the program folder and across updates.
fn default_home() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("library").is_dir() || cwd.join("data").is_dir() {
        return cwd;
    }
    match std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("XDG_DATA_HOME")) {
        Some(dir) => PathBuf::from(dir).join("Vinyl"),
        None => cwd,
    }
}

/// Sets up the directory layout. `MUSIC_HOME` (or the current directory) holds
/// `library/`, `data/` and `static/`, exactly like the original app.
pub fn init() -> &'static Paths {
    PATHS.get_or_init(|| {
        let base = std::env::var_os("MUSIC_HOME").map(PathBuf::from).unwrap_or_else(default_home);
        let _ = fs::create_dir_all(&base);
        let base = fs::canonicalize(&base).unwrap_or(base);
        let base = PathBuf::from(base.to_string_lossy().trim_start_matches(r"\\?\").to_string());
        let library = base.join("library");
        let data = base.join("data");
        let _ = fs::create_dir_all(&library);
        let _ = fs::create_dir_all(&data);
        Paths {
            covers: library.join("covers"),
            static_dir: base.join("static"),
            playlists: data.join("playlists.json"),
            stats: data.join("stats.json"),
            likes: data.join("likes.json"),
            history: data.join("history.jsonl"), // one JSON object per line, appended to on every play
            settings: data.join("settings.json"),
            sources: data.join("sources.json"),
            loudness: data.join("loudness.json"),
            meta: data.join("metadata.json"),
            player_state: data.join("player_state.json"),
            playlist_covers: data.join("playlist_covers"),
            lyrics: data.join("lyrics"),
            library,
            data,
            base,
        }
    })
}

pub fn paths() -> &'static Paths {
    init()
}

/// The global lock guarding read-modify-write cycles on the JSON files.
/// Not re-entrant: never call something that takes it while holding it.
static LOCK: Mutex<()> = Mutex::new(());

pub fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ---------------- logging ----------------

fn log_line(level: &str, msg: &str) {
    let p = paths();
    let _ = fs::create_dir_all(&p.data);
    let file = p.data.join("app.log");
    // keep the log small: roll it over at ~500 KB
    if fs::metadata(&file).map(|m| m.len() > 500_000).unwrap_or(false) {
        let _ = fs::rename(&file, p.data.join("app.log.1"));
    }
    let line = format!("{:.0} {} {}\n", now(), level, msg);
    match fs::OpenOptions::new().create(true).append(true).open(&file) {
        Ok(mut f) => {
            let _ = f.write_all(line.as_bytes());
        }
        Err(_) => eprint!("{line}"),
    }
}

pub fn info(msg: impl AsRef<str>) {
    log_line("INFO", msg.as_ref());
}
pub fn warn(msg: impl AsRef<str>) {
    log_line("WARNING", msg.as_ref());
}
pub fn error(msg: impl AsRef<str>) {
    log_line("ERROR", msg.as_ref());
}

// ---------------- JSON files ----------------

pub fn write_json(path: &Path, data: &Value, pretty: bool, backup: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let tmp = path.with_file_name(format!("{name}.{}.tmp", &uuid::Uuid::new_v4().simple().to_string()[..8]));
    let text = if pretty {
        serde_json::to_string_pretty(data)
    } else {
        serde_json::to_string(data)
    }
    .unwrap_or_else(|_| "null".into());
    let result = (|| {
        fs::write(&tmp, text)?;
        if backup && path.is_file() {
            if let Err(e) = fs::copy(path, path.with_file_name(format!("{name}.bak"))) {
                warn(format!("Couldn't back up {name}: {e}"));
            }
        }
        fs::rename(&tmp, path)
    })();
    let _ = fs::remove_file(&tmp);
    if result.is_ok() && name == "metadata.json" {
        META_VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    if result.is_ok() && matches!(name.as_str(), "stats.json" | "metadata.json" | "loudness.json") {
        DATA_VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    result
}

/// Rewrites every JSON file in `dir` (and its subfolders) with indentation, leaving
/// files that don't parse untouched. Returns how many files were rewritten.
pub fn reformat_json_dir(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += reformat_json_dir(&path);
        } else if path.extension().is_some_and(|e| e == "json") {
            let parsed = fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok());
            match parsed {
                Some(v) => match write_json(&path, &v, true, false) {
                    Ok(()) => count += 1,
                    Err(e) => warn(format!("Couldn't reformat {}: {e}", path.display())),
                },
                None => warn(format!("Skipped {} (not valid JSON)", path.display())),
            }
        }
    }
    count
}

/// Saves a pretty, backed-up JSON file, logging (not propagating) failures.
pub fn save(path: &Path, data: &Value) {
    if let Err(e) = write_json(path, data, true, true) {
        error(format!("Couldn't write {}: {e}", path.display()));
    }
}

/// The JSON stored at `path`, falling back to its `.bak` if it's unreadable
/// (the bad file is renamed `*.corrupt` so it isn't backed up over the good copy).
pub fn read_json(path: &Path, default: Value) -> Value {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let bak = path.with_file_name(format!("{name}.bak"));
    for candidate in [path.to_path_buf(), bak] {
        if !candidate.is_file() {
            continue;
        }
        let parsed = fs::read_to_string(&candidate)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).map_err(|e| e.to_string()));
        match parsed {
            Ok(v) => return v,
            Err(e) => {
                let is_main = candidate == path;
                error(format!(
                    "Unreadable {} ({e}){}",
                    candidate.file_name().unwrap_or_default().to_string_lossy(),
                    if is_main { "; trying its backup" } else { "" }
                ));
                if is_main {
                    let _ = fs::rename(path, path.with_file_name(format!("{name}.corrupt")));
                }
            }
        }
    }
    default
}

pub fn read_obj(path: &Path) -> Value {
    let v = read_json(path, Value::Object(Default::default()));
    if v.is_object() { v } else { Value::Object(Default::default()) }
}

pub fn read_arr(path: &Path) -> Value {
    let v = read_json(path, Value::Array(vec![]));
    if v.is_array() { v } else { Value::Array(vec![]) }
}
