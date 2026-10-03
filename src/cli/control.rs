//! stop / pause / resume（FindWindow + PostMessage）

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW, WM_CLOSE};

use super::{find_daemon, wait_until};
use crate::common::{clock, console};
use crate::daemon::{WM_PAUSE, WM_RESUME};

fn daemon_or_exit() -> Result<(HWND, u32), i32> {
    find_daemon().ok_or_else(|| {
        console::out("Not running");
        1
    })
}

fn post(hwnd: HWND, msg: u32, wparam: usize) -> bool {
    unsafe { PostMessageW(hwnd, msg, wparam, 0) != 0 }
}

pub fn stop() -> i32 {
    let (hwnd, pid) = match daemon_or_exit() {
        Ok(d) => d,
        Err(code) => return code,
    };
    if !post(hwnd, WM_CLOSE, 0) {
        console::err("Could not ask the recorder to stop");
        return 10;
    }
    let hwnd_val = hwnd as usize;
    if wait_until(10_000, || unsafe { IsWindow(hwnd_val as HWND) } == 0) {
        console::out(&format!("Stopped (PID {pid})"));
        0
    } else {
        console::err(&format!("Could not confirm that the recorder stopped (PID {pid})"));
        10
    }
}

pub fn pause(arg: Option<&str>) -> i32 {
    let minutes = match arg {
        None => 0,
        Some(a) => match clock::parse_minutes(a) {
            Some(m) => m,
            None => {
                console::err(&format!("Invalid duration: {a} (examples: 30m, 2h, 1h30m)"));
                return 3;
            }
        },
    };
    let (hwnd, _) = match daemon_or_exit() {
        Ok(d) => d,
        Err(code) => return code,
    };
    if !post(hwnd, WM_PAUSE, minutes as usize) {
        console::err("Could not ask the recorder to pause");
        return 10;
    }
    if minutes == 0 {
        console::out("Paused (until you run: fuashiato resume)");
    } else {
        let until = clock::now() + chrono::TimeDelta::minutes(minutes as i64);
        console::out(&format!(
            "Paused for {}, until {}",
            clock::fmt_duration(chrono::TimeDelta::minutes(minutes as i64)),
            until.format("%H:%M")
        ));
    }
    0
}

pub fn resume() -> i32 {
    let (hwnd, _) = match daemon_or_exit() {
        Ok(d) => d,
        Err(code) => return code,
    };
    if !post(hwnd, WM_RESUME, 0) {
        console::err("Could not ask the recorder to resume");
        return 10;
    }
    console::out("Resumed");
    0
}
