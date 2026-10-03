//! WTSセッション通知（ロック、リモート切断）

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT,
    WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

use crate::common::errlog;

pub enum Change {
    Lock,
    Unlock,
}

pub fn register(hwnd: HWND) {
    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
        errlog::log_win32("WTSRegisterSessionNotification");
    }
}

pub fn unregister(hwnd: HWND) {
    unsafe { WTSUnRegisterSessionNotification(hwnd) };
}

/// `WM_WTSSESSION_CHANGE` の wParam を解釈する
pub fn classify(wparam: usize) -> Option<Change> {
    match wparam as u32 {
        WTS_SESSION_LOCK | WTS_REMOTE_DISCONNECT | WTS_CONSOLE_DISCONNECT => Some(Change::Lock),
        WTS_SESSION_UNLOCK | WTS_REMOTE_CONNECT | WTS_CONSOLE_CONNECT => Some(Change::Unlock),
        _ => None,
    }
}
