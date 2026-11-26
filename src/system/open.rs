//! Opening things outside the app: web links in the default browser, and Vinyl's own folders
//! in Explorer. Only http(s) links and the two known folders are accepted, never a path or
//! command from the request, so the page can't use this to run anything else.

use crate::base::store::paths;

/// Opens a web link in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    let web = url.starts_with("https://") || url.starts_with("http://");
    let clean = url.len() <= 2000 && !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"');
    if !web || !clean {
        return Err("Only web links can be opened".into());
    }
    launch(url)
}

/// Opens one of Vinyl's folders: "data" (settings, playlists, history) or "library" (the songs).
pub fn open_folder(which: &str) -> Result<(), String> {
    let p = paths();
    let dir = match which {
        "data" => &p.data,
        "library" => &p.library,
        _ => return Err("Unknown folder".into()),
    };
    launch(&dir.to_string_lossy())
}

fn launch(target: &str) -> Result<(), String> {
    // explorer.exe hands a URL to the default browser and shows a folder; it is not waited on
    // (it returns as soon as the window is asked for, and its exit code means nothing).
    let program = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(program).arg(target).spawn().map(|_| ()).map_err(|e| format!("Couldn't open it ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_web_links_are_opened() {
        for bad in ["file:///C:/Windows", "javascript:alert(1)", "C:\\Windows\\system32\\calc.exe", "https://a.com/\" & calc", "https://a.com/x y", "/select,C:\\"] {
            assert!(open_url(bad).is_err(), "{bad} should be refused");
        }
    }
}
