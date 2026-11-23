//! Keyboard media keys (play/pause, next, previous, stop), even while Vinyl is in the background.
//! A thread registers them as system-wide hotkeys and queues the presses; the page long-polls
//! `/api/media/poll` and acts on them. Only the desktop window does this: in a browser the page
//! gets the keys through the browser's own media-session support.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

static ENABLED: AtomicBool = AtomicBool::new(false);
static QUEUE: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static SIGNAL: Condvar = Condvar::new();

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn push(key: &'static str) {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner()).push(key);
    SIGNAL.notify_all();
}

/// Waits up to `timeout` for key presses and returns (and clears) the ones that arrived.
pub fn wait(timeout: Duration) -> Vec<&'static str> {
    let mut q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    if q.is_empty() {
        q = SIGNAL.wait_timeout(q, timeout).unwrap_or_else(|e| e.into_inner()).0;
    }
    std::mem::take(&mut *q)
}

#[cfg(windows)]
pub fn start() {
    #[repr(C)]
    struct Msg {
        hwnd: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt_x: i32,
        pt_y: i32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterHotKey(hwnd: isize, id: i32, modifiers: u32, vk: u32) -> i32;
        fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    }
    const WM_HOTKEY: u32 = 0x0312;
    const MOD_NOREPEAT: u32 = 0x4000;
    // (hotkey id, virtual key, name the page understands)
    const KEYS: [(i32, u32, &str); 4] = [(1, 0xB3, "play_pause"), (2, 0xB0, "next"), (3, 0xB1, "prev"), (4, 0xB2, "stop")];

    std::thread::spawn(|| {
        // A hotkey belongs to the thread that registered it, and that thread must pump messages.
        let mut any = false;
        for (id, vk, _) in KEYS {
            if unsafe { RegisterHotKey(0, id, MOD_NOREPEAT, vk) } != 0 {
                any = true;
            }
        }
        if !any {
            return; // another app owns the media keys; the browser's media session still works
        }
        ENABLED.store(true, Ordering::Relaxed);
        let mut msg = Msg { hwnd: 0, message: 0, wparam: 0, lparam: 0, time: 0, pt_x: 0, pt_y: 0 };
        while unsafe { GetMessageW(&mut msg, 0, 0, 0) } > 0 {
            if msg.message == WM_HOTKEY {
                if let Some((_, _, name)) = KEYS.iter().find(|(id, _, _)| *id as usize == msg.wparam) {
                    push(name);
                }
            }
        }
    });
}

#[cfg(not(windows))]
pub fn start() {}
