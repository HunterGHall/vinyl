#![cfg_attr(all(windows, feature = "desktop", not(debug_assertions)), windows_subsystem = "windows")]
//! Vinyl: a local music player, ported from the Flask + pywebview app.
//!
//! Download tracks or whole playlists from YouTube/Spotify links, organise them
//! into playlists and play them back - all stored locally. Needs `yt-dlp`,
//! `ffmpeg` and `ffprobe` on PATH.
//!
//!   music-rs          open the desktop window
//!   music-rs --web    just serve http://127.0.0.1:8735 for a browser
//!
//! The library and data live in `$MUSIC_HOME` (default: the current directory).

mod deps;
mod discover;
mod download;
mod library;
mod mediakeys;
mod lyrics;
mod routes;
mod spotify;
mod store;
mod tools;
mod update;
mod util;

const DEFAULT_PORT: u16 = 8735;

/// The port to serve on: `VINYL_PORT` if set, otherwise 8735.
fn port() -> u16 {
    std::env::var("VINYL_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

fn run_web() -> Result<(), String> {
    library::start_background_tasks();
    library::scan_loudness_async();
    tools::start_sync_thread();
    routes::serve(port())
}

fn wait_for_server() {
    for _ in 0..60 {
        if std::net::TcpStream::connect(("127.0.0.1", port())).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(all(feature = "desktop", windows))]
fn name_process_for_windows() {
    // Name the process for the Windows media overlay instead of "Unknown app":
    // Windows looks the label up by AppUserModelID, so register a DisplayName
    // for ours under HKCU (per-user, no admin needed).
    const APP_ID: &str = "local.vinyl.player";
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
    }
    let _ = util::command("reg")
        .args(["add", &format!(r"HKCU\Software\Classes\AppUserModelId\{APP_ID}")])
        .args(["/v", "DisplayName", "/t", "REG_SZ", "/d", "Vinyl", "/f"])
        .output();
    let wide: Vec<u16> = APP_ID.encode_utf16().chain(Some(0)).collect();
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
    }
}

/// The window/taskbar icon, decoded from the embedded 256 px PNG.
#[cfg(feature = "desktop")]
fn load_icon() -> Option<tao::window::Icon> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../assets/logo/icon-256.png")[..]));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    tao::window::Icon::from_rgba(buf, info.width, info.height).ok()
}

#[cfg(feature = "desktop")]
fn run_desktop() -> Result<(), String> {
    use tao::dpi::{LogicalSize, Size};
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;
    use wry::{WebContext, WebViewBuilder};

    #[cfg(windows)]
    name_process_for_windows();
    mediakeys::start();

    std::thread::spawn(|| {
        if let Err(e) = run_web() {
            store::error(e);
        }
    });
    wait_for_server();

    let event_loop = EventLoop::new();
    let icon = load_icon();
    let window = WindowBuilder::new()
        .with_title("Vinyl")
        .with_window_icon(icon)
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(Size::Logical(LogicalSize::new(960.0, 620.0)))
        .build(&event_loop)
        .map_err(|e| e.to_string())?;

    // A persistent (non-incognito) WebView2 profile makes Windows show track
    // metadata in the media overlay.
    let mut context = WebContext::new(Some(store::paths().data.join("webview")));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(format!("http://127.0.0.1:{}", port()))
        .with_background_color((11, 11, 13, 255));

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let _webview = builder.build(&window).map_err(|e| e.to_string())?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().ok_or("no gtk window")?).map_err(|e| e.to_string())?
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep_alive = (&_webview, &window);
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *control_flow = ControlFlow::Exit;
        }
    });
}

#[cfg(not(feature = "desktop"))]
fn run_desktop() -> Result<(), String> {
    run_web()
}

fn main() {
    store::init();
    let web_only = std::env::args().any(|a| a == "--web");
    if web_only && std::env::var_os("VINYL_MEDIA_KEYS").is_some() {
        mediakeys::start(); // testing aid: the desktop window starts this itself
    }
    let result = if web_only { run_web() } else { run_desktop() };
    if let Err(e) = result {
        eprintln!("{e}");
        store::error(&e);
        std::process::exit(1);
    }
}
