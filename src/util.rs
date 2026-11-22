//! Small helpers: text munging, HTTP with retries, subprocess setup.

use crate::store;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use regex_lite::Regex;
use std::io::Read;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";

/// Everything except letters, digits and `-._~/` is escaped (like Python's `quote`).
const QUOTE_SET: &AsciiSet = &CONTROLS
    .add(b' ').add(b'!').add(b'"').add(b'#').add(b'$').add(b'%').add(b'&').add(b'\'')
    .add(b'(').add(b')').add(b'*').add(b'+').add(b',').add(b':').add(b';').add(b'<')
    .add(b'=').add(b'>').add(b'?').add(b'@').add(b'[').add(b'\\').add(b']').add(b'^')
    .add(b'`').add(b'{').add(b'|').add(b'}');

pub fn quote(s: &str) -> String {
    utf8_percent_encode(s, QUOTE_SET).to_string()
}

/// Like `quote` but `/` is escaped too (for query-string values).
pub fn quote_all(s: &str) -> String {
    quote(s).replace('/', "%2F")
}

pub fn unquote(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()
}

macro_rules! lazy_re {
    ($name:ident, $pat:expr) => {
        pub static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

lazy_re!(BAD_FILENAME_CHARS, r#"[\\/:*?"<>|]"#);

/// Letters and digits only, lower-cased - to tell punctuation changes from real ones.
pub fn words(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

pub fn sanitize_filename(name: &str) -> String {
    let s = BAD_FILENAME_CHARS.replace_all(name, "_");
    let s = s.trim();
    if s.is_empty() { "track".to_string() } else { s.to_string() }
}

/// Truncates to at most `n` characters.
pub fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub fn is_spotify_url(url: &str) -> bool {
    url.contains("spotify.com") || url.starts_with("spotify:")
}

pub fn is_spotify_playlist_url(url: &str) -> bool {
    is_spotify_url(url) && url.contains("/playlist/")
}

/// A subprocess that won't flash a console window on Windows.
pub fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
    // yt-dlp and ffmpeg are often installed where an app started from the Start menu doesn't look
    // (winget's folders, Chocolatey, Scoop...), so add the places that hold them to its PATH.
    let extra = crate::deps::extra_dirs();
    if !extra.is_empty() {
        let mut paths: Vec<std::path::PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        for d in extra {
            if !paths.contains(&d) {
                paths.push(d);
            }
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            c.env("PATH", joined);
        }
    }
    c
}

/// One HTTP client for the whole app (TLS via the operating system's own stack).
pub fn agent() -> &'static ureq::Agent {
    static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
        let mut builder = ureq::AgentBuilder::new();
        if let Ok(tls) = native_tls::TlsConnector::new() {
            builder = builder.tls_connector(std::sync::Arc::new(tls));
        }
        builder.build()
    });
    &AGENT
}

const RETRY_CODES: [u16; 5] = [429, 500, 502, 503, 504];

fn read_body(resp: ureq::Response) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    resp.into_reader()
        .take(50_000_000)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// GET with a few retries and backoff for the hiccups that are normal on the
/// open web (timeouts, rate limiting, brief 5xx). Returns the body bytes.
pub fn http_get(url: &str, timeout: u64, retries: u32) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for attempt in 0..retries {
        let result = agent().get(url)
            .set("User-Agent", USER_AGENT)
            .set("Accept-Language", "en")
            .timeout(Duration::from_secs(timeout))
            .call();
        match result {
            Ok(resp) => return read_body(resp),
            Err(ureq::Error::Status(code, _)) if !RETRY_CODES.contains(&code) => {
                return Err(format!("HTTP Error {code}"));
            }
            Err(e) => last = e.to_string(),
        }
        if attempt + 1 < retries {
            std::thread::sleep(Duration::from_millis((1500u64) << attempt));
        }
    }
    Err(last)
}

pub fn http_get_text(url: &str, timeout: u64, retries: u32) -> Result<String, String> {
    http_get(url, timeout, retries).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Best-effort - cover art is a nice-to-have, never fails the download.
pub fn download_image(url: &str, dest: &std::path::Path) {
    match http_get(url, 15, 1) {
        Ok(data) => {
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(dest, data) {
                store::warn(format!("Couldn't save cover image: {e}"));
            }
        }
        Err(e) => store::warn(format!("Couldn't download cover image: {e}")),
    }
}
