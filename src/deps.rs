//! The command-line tools Vinyl uses to add music: yt-dlp (download) and ffmpeg/ffprobe (convert
//! and measure). An app started from the Start menu often doesn't have the folders winget put
//! them in on its PATH, so this looks in the usual places too, and can install what is missing.

use crate::util;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const TOOLS: [&str; 3] = ["yt-dlp", "ffmpeg", "ffprobe"];

fn exe(dir: &Path, name: &str) -> PathBuf {
    dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() })
}

fn has_tool(dir: &Path) -> bool {
    TOOLS.iter().any(|t| exe(dir, t).is_file())
}

/// Directories under `dir` (up to `depth` levels) that hold one of the tools.
fn scan(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if has_tool(dir) {
        out.push(dir.to_path_buf());
    }
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan(&p, depth - 1, out);
        }
    }
}

fn compute_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let env_dir = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut fixed: Vec<PathBuf> = Vec::new();
    if let Some(local) = env_dir("LOCALAPPDATA") {
        fixed.push(local.join("Microsoft").join("WinGet").join("Links"));
        fixed.push(local.join("Vinyl").join("tools"));
        // winget unpacks portable packages (ffmpeg, yt-dlp) into per-package folders
        let packages = local.join("Microsoft").join("WinGet").join("Packages");
        if let Ok(rd) = std::fs::read_dir(&packages) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                if name.contains("ffmpeg") || name.contains("yt-dlp") || name.contains("yt.dlp") {
                    scan(&e.path(), 4, &mut out);
                }
            }
        }
    }
    if let Some(p) = env_dir("ProgramFiles") {
        fixed.push(p.join("ffmpeg").join("bin"));
    }
    if let Some(p) = env_dir("ProgramData") {
        fixed.push(p.join("chocolatey").join("bin"));
    }
    if let Some(p) = env_dir("USERPROFILE") {
        fixed.push(p.join("scoop").join("shims"));
    }
    fixed.push(PathBuf::from(r"C:\ffmpeg\bin"));
    for d in fixed {
        if has_tool(&d) {
            out.push(d);
        }
    }
    out.dedup();
    out
}

static CACHE: LazyLock<Mutex<Option<(Instant, Vec<PathBuf>)>>> = LazyLock::new(Default::default);

fn invalidate() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Extra folders (beyond PATH) that contain yt-dlp / ffmpeg / ffprobe, cached for a minute.
pub fn extra_dirs() -> Vec<PathBuf> {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, dirs)) = c.as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return dirs.clone();
        }
    }
    let dirs = compute_dirs();
    *c = Some((Instant::now(), dirs.clone()));
    dirs
}

/// Where `name` is installed: on PATH, or in one of the usual places.
pub fn find(name: &str) -> Option<PathBuf> {
    let on_path: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    on_path.into_iter().chain(extra_dirs()).map(|d| exe(&d, name)).find(|p| p.is_file())
}

static STATE: LazyLock<Mutex<Value>> = LazyLock::new(|| Mutex::new(json!({"installing": false, "error": null})));

fn set_state(installing: bool, error: Option<String>) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = json!({"installing": installing, "error": error});
}

pub fn status() -> Value {
    let st = STATE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    json!({
        "yt_dlp": find("yt-dlp").is_some(),
        "ffmpeg": find("ffmpeg").is_some() && find("ffprobe").is_some(),
        "winget": find("winget").is_some() || util::command("winget").arg("--version").output().is_ok(),
        "installing": st["installing"],
        "error": st["error"],
    })
}

/// Installs whatever is missing with winget, in the background. False if already running.
pub fn install() -> bool {
    if STATE.lock().unwrap_or_else(|e| e.into_inner())["installing"] == json!(true) {
        return false;
    }
    set_state(true, None);
    std::thread::spawn(|| {
        let mut failed: Vec<&str> = Vec::new();
        let want = [("yt-dlp", "yt-dlp.yt-dlp", find("yt-dlp").is_some()), ("ffmpeg", "Gyan.FFmpeg", find("ffmpeg").is_some() && find("ffprobe").is_some())];
        for (label, id, present) in want {
            if present {
                continue;
            }
            let ok = util::command("winget")
                .args(["install", "--id", id, "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            invalidate();
            if !ok || find(label).is_none() {
                failed.push(label);
            }
        }
        invalidate();
        if failed.is_empty() {
            set_state(false, None);
        } else {
            set_state(false, Some(format!("Couldn't install {}. Install it manually (winget install {}).", failed.join(" and "), if failed.contains(&"ffmpeg") { "Gyan.FFmpeg" } else { "yt-dlp.yt-dlp" })));
        }
    });
    true
}

/// Turns the tools' raw failures into something a person can act on.
pub fn friendly(err: &str) -> String {
    let l = err.to_lowercase();
    if l.contains("ffmpeg") && (l.contains("not found") || l.contains("not installed") || l.contains("ffprobe")) {
        "ffmpeg is missing, so Vinyl can't convert the audio. Open Settings, then Download tools, and install it.".into()
    } else if l.contains("couldn't run yt-dlp") {
        "yt-dlp is missing, so Vinyl can't download songs. Open Settings, then Download tools, and install it.".into()
    } else {
        err.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_missing_tools() {
        assert!(friendly("Postprocessing: ffprobe and ffmpeg not found. Please install or provide the path using --ffmpeg-location").starts_with("ffmpeg is missing"));
        assert!(friendly("Couldn't run yt-dlp (program not found)").starts_with("yt-dlp is missing"));
        assert_eq!(friendly("HTTP Error 403"), "HTTP Error 403");
    }
}
