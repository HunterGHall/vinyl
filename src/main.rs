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

#[cfg(feature = "desktop")]
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
    const APP_ID: &str = "HunterGHall.Vinyl"; // must match AppUserModelID in installer/vinyl.iss
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

/// Where the window was last time (outer position and inner size in physical pixels), so it
/// reopens in the same place at the same size.
#[cfg(feature = "desktop")]
#[derive(Clone, Copy)]
struct SavedWindow {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    maximized: bool,
}

#[cfg(feature = "desktop")]
impl SavedWindow {
    fn path() -> std::path::PathBuf {
        store::paths().data.join("window_state.json")
    }
    fn load() -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(Self::path()).ok()?).ok()?;
        let n = |k: &str| v[k].as_i64();
        let (w, h) = (n("w")?, n("h")?);
        if !(300..=20000).contains(&w) || !(200..=20000).contains(&h) {
            return None;
        }
        Some(Self { x: n("x")? as i32, y: n("y")? as i32, w: w as u32, h: h as u32, maximized: v["maximized"].as_bool().unwrap_or(false) })
    }
    fn save(&self) {
        let v = serde_json::json!({"x": self.x, "y": self.y, "w": self.w, "h": self.h, "maximized": self.maximized});
        let _ = std::fs::write(Self::path(), v.to_string());
    }
}

#[cfg(feature = "desktop")]
fn run_desktop() -> Result<(), String> {
    use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize, Size};
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
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

    // The page draws its own title bar (drag area + minimize/maximize/close) and tells us what
    // the user did through the webview's IPC channel.
    #[derive(Clone, Copy)]
    enum Cmd {
        Drag,
        Minimize,
        ToggleMaximize,
        Close,
    }
    let event_loop = EventLoopBuilder::<Cmd>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let icon = load_icon();
    let mut builder = WindowBuilder::new()
        .with_title("Vinyl")
        .with_window_icon(icon)
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(Size::Logical(LogicalSize::new(960.0, 620.0)));
    // Reopen where it was last time, unless that spot is no longer on any screen (monitor unplugged).
    let restored = SavedWindow::load().filter(|sw| {
        let (cx, cy) = (sw.x as i64 + sw.w as i64 / 2, sw.y as i64 + 40);
        event_loop.available_monitors().any(|m| {
            let (p, sz) = (m.position(), m.size());
            cx >= p.x as i64 && cx < p.x as i64 + sz.width as i64 && cy >= p.y as i64 && cy < p.y as i64 + sz.height as i64
        })
    });
    if let Some(sw) = restored {
        builder = builder
            .with_inner_size(PhysicalSize::new(sw.w, sw.h))
            .with_position(PhysicalPosition::new(sw.x, sw.y))
            .with_maximized(sw.maximized);
    }
    #[cfg(windows)]
    let builder = {
        use tao::platform::windows::WindowBuilderExtWindows;
        builder.with_undecorated_shadow(true) // keeps the drop shadow and rounded corners
    };
    let window = builder.build(&event_loop).map_err(|e| e.to_string())?;

    // A persistent (non-incognito) WebView2 profile makes Windows show track
    // metadata in the media overlay.
    let mut context = WebContext::new(Some(store::paths().data.join("webview")));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_ipc_handler(move |req| {
            let cmd = match req.body().as_str() {
                "drag" => Cmd::Drag,
                "min" => Cmd::Minimize,
                "max" => Cmd::ToggleMaximize,
                "close" => Cmd::Close,
                _ => return,
            };
            let _ = proxy.send_event(cmd);
        })
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

    // Remember the window's place. Moves and resizes only mark it as changed; once things have been
    // quiet for a moment (so the OS has finished maximizing, snapping, etc.) it reads the final state.
    // A maximized window keeps the size it had before, so un-maximizing goes back to that.
    let mut saved = restored.unwrap_or(SavedWindow { x: 0, y: 0, w: 0, h: 0, maximized: false });
    let mut changed_at: Option<std::time::Instant> = Some(std::time::Instant::now());
    const SETTLE: std::time::Duration = std::time::Duration::from_millis(500);
    let capture = move |window: &tao::window::Window, saved: &mut SavedWindow| {
        if window.is_minimized() {
            return;
        }
        saved.maximized = window.is_maximized();
        let size = window.inner_size();
        if !saved.maximized && size.width > 0 && size.height > 0 {
            if let Ok(p) = window.outer_position() {
                (saved.x, saved.y, saved.w, saved.h) = (p.x, p.y, size.width, size.height);
            }
        }
        if saved.w > 0 {
            saved.save();
        }
    };

    event_loop.run(move |event, _, control_flow| {
        let _keep_alive = (&_webview, &window);
        match event {
            Event::WindowEvent { event: WindowEvent::Moved(_) | WindowEvent::Resized(_), .. } => {
                changed_at = Some(std::time::Instant::now());
            }
            Event::MainEventsCleared => {
                if changed_at.is_some_and(|t| t.elapsed() >= SETTLE) {
                    capture(&window, &mut saved);
                    changed_at = None;
                }
            }
            Event::LoopDestroyed => capture(&window, &mut saved),
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } | Event::UserEvent(Cmd::Close) => {
                capture(&window, &mut saved);
                *control_flow = ControlFlow::Exit;
                return;
            }
            Event::UserEvent(Cmd::Drag) => {
                let _ = window.drag_window();
            }
            Event::UserEvent(Cmd::Minimize) => window.set_minimized(true),
            Event::UserEvent(Cmd::ToggleMaximize) => window.set_maximized(!window.is_maximized()),
            _ => {}
        }
        // while a change is waiting to settle, wake up when it should have
        *control_flow = match changed_at {
            Some(t) => ControlFlow::WaitUntil(t + SETTLE + std::time::Duration::from_millis(20)),
            None => ControlFlow::Wait,
        };
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
