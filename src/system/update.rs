//! Self-update: on start the app asks GitHub for the latest release of the repo. When it is
//! newer than this build, the UI offers to install it: the release's `vinyl-setup.exe` is
//! downloaded, checked against GitHub's SHA-256 for the asset, and run (the installer
//! closes this app, replaces it and starts the new version).

use crate::base::util;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

/// `owner/name` of the GitHub repository whose releases carry the installer.
const REPO: &str = "HunterGHall/vinyl";
const ASSET: &str = "vinyl-setup.exe";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn releases_url() -> String {
    // VINYL_RELEASES_URL lets a test point the updater at a local mock.
    std::env::var("VINYL_RELEASES_URL").unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases/latest"))
}

/// "v1.2.3" / "1.2" -> [1, 2, 3]
fn parse_version(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse_version(latest), parse_version(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// Only ever download from GitHub itself.
fn trusted_download(url: &str) -> bool {
    if std::env::var_os("VINYL_RELEASES_URL").is_some() {
        return true; // test mode
    }
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split('/').next().unwrap_or("");
    ["github.com", "githubusercontent.com"].iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// What the latest release looks like, and whether it is newer than this build.
pub fn check() -> Result<Value, String> {
    let resp = util::agent()
        .get(&releases_url())
        .set("User-Agent", "Vinyl-updater")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "No release has been published yet.".to_string(),
            other => format!("Couldn't check for updates: {other}"),
        })?;
    let mut body = String::new();
    resp.into_reader().take(2_000_000).read_to_string(&mut body).map_err(|e| e.to_string())?;
    let rel: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = rel["tag_name"].as_str().unwrap_or("").to_string();
    let asset = rel["assets"].as_array().into_iter().flatten().find(|a| a["name"] == ASSET);
    let available = !tag.is_empty() && is_newer(&tag, VERSION) && asset.is_some();
    Ok(json!({
        "current": VERSION,
        "latest": tag.trim_start_matches(['v', 'V']),
        "available": available,
        "notes": rel["body"].as_str().unwrap_or(""),
        "page": rel["html_url"],
        "url": asset.map(|a| a["browser_download_url"].clone()).unwrap_or(Value::Null),
        "size": asset.map(|a| a["size"].clone()).unwrap_or(Value::Null),
        "digest": asset.map(|a| a["digest"].clone()).unwrap_or(Value::Null),
    }))
}

static STATE: LazyLock<Mutex<Value>> = LazyLock::new(|| Mutex::new(json!({"status": "idle"})));

fn set_state(v: Value) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = v;
}

pub fn status() -> Value {
    let mut v = STATE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    v["current"] = json!(VERSION);
    v
}

/// Downloads the installer for the latest release and runs it. Returns false if one is already running.
pub fn install() -> bool {
    {
        let st = status();
        if st["status"] == "downloading" || st["status"] == "launching" {
            return false;
        }
    }
    set_state(json!({"status": "downloading", "done": 0, "total": 0}));
    std::thread::spawn(|| {
        if let Err(e) = download_and_run() {
            crate::base::store::error(format!("Update failed: {e}"));
            set_state(json!({"status": "error", "error": e}));
        }
    });
    true
}

fn download_and_run() -> Result<(), String> {
    let info = check()?;
    if info["available"] != json!(true) {
        return Err("You already have the latest version.".into());
    }
    let url = info["url"].as_str().ok_or("The release has no installer.")?.to_string();
    if !trusted_download(&url) {
        return Err("Refusing to download from an untrusted address.".into());
    }
    let digest = info["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(|d| d.to_lowercase());

    let resp = util::agent()
        .get(&url)
        .set("User-Agent", "Vinyl-updater")
        .timeout(Duration::from_secs(900))
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = resp.header("Content-Length").and_then(|v| v.parse::<u64>().ok()).or(info["size"].as_u64()).unwrap_or(0);
    let dir = std::env::temp_dir().join("vinyl-update");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(ASSET);
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut reader = resp.into_reader();
    let (mut buf, mut done, mut hasher) = (vec![0u8; 64 * 1024], 0u64, Sha256::new());
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        done += n as u64;
        set_state(json!({"status": "downloading", "done": done, "total": total}));
    }
    drop(file);
    if let Some(want) = digest {
        let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != want {
            let _ = std::fs::remove_file(&path);
            return Err("The download is corrupt (checksum mismatch). Nothing was installed.".into());
        }
    }

    set_state(json!({"status": "launching"}));
    util::command(&path.to_string_lossy())
        .args(["/SILENT", "/NOCANCEL", "/CLOSEAPPLICATIONS", "/RESTARTAPPLICATIONS"])
        .spawn()
        .map_err(|e| format!("Couldn't start the installer: {e}"))?;
    // Give the page a moment to show "installing", then get out of the installer's way.
    std::thread::sleep(Duration::from_millis(1500));
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v1.0.1", "1.0.0"));
        assert!(is_newer("1.1", "1.0.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        assert!(is_newer("1.0.0.1", "1.0.0"));
    }

    #[test]
    fn only_trusts_github() {
        assert!(trusted_download("https://github.com/o/r/releases/download/v1/vinyl-setup.exe"));
        assert!(trusted_download("https://objects.githubusercontent.com/x"));
        assert!(!trusted_download("https://evil.example.com/github.com/x"));
        assert!(!trusted_download("http://github.com/x"));
        assert!(!trusted_download("https://notgithub.com/x"));
    }
}
