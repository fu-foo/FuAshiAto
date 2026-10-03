//! 前面ウィンドウ、タイトル、プロセス名の取得

use std::collections::HashMap;

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId,
};

use super::recorder::Win;

const FRAME_HOST: &str = "ApplicationFrameHost.exe";
/// PIDの再利用で古い名前が残らないよう、キャッシュはこの件数を超えたら捨てる
const CACHE_MAX: usize = 256;

pub struct Foreground {
    self_pid: u32,
    names: HashMap<u32, String>,
}

impl Foreground {
    pub fn new() -> Self {
        Foreground { self_pid: std::process::id(), names: HashMap::new() }
    }

    /// 現在の前面ウィンドウ。前面がない、または自プロセスなら `None`。
    pub fn read(&mut self) -> Option<Win> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return None;
        }
        let pid = window_pid(hwnd);
        if pid == self.self_pid {
            return None;
        }
        let title = window_text(hwnd);
        let mut proc = self.proc_name(pid);
        if proc.eq_ignore_ascii_case(FRAME_HOST) {
            // UWPアプリ：子ウィンドウから実体のプロセスを探す
            if let Some(real) = find_uwp_child_pid(hwnd, pid) {
                if real == self.self_pid {
                    return None;
                }
                let name = self.proc_name(real);
                if !name.is_empty() {
                    proc = name;
                }
            }
        }
        Some(Win { proc, title })
    }

    fn proc_name(&mut self, pid: u32) -> String {
        if pid == 0 {
            return String::new();
        }
        if let Some(n) = self.names.get(&pid) {
            return n.clone();
        }
        if self.names.len() >= CACHE_MAX {
            self.names.clear();
        }
        let name = query_proc_name(pid);
        self.names.insert(pid, name.clone());
        name
    }
}

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

fn window_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// プロセスのexeファイル名。取得できなければ空文字列。
fn query_proc_name(pid: u32) -> String {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit('\\').next().unwrap_or(&full).to_string()
    }
}

struct ChildSearch {
    frame_pid: u32,
    found: u32,
}

unsafe extern "system" fn enum_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam as *mut ChildSearch);
    let pid = window_pid(hwnd);
    if pid != 0 && pid != search.frame_pid {
        search.found = pid;
        return 0; // 列挙を止める
    }
    1
}

fn find_uwp_child_pid(hwnd: HWND, frame_pid: u32) -> Option<u32> {
    let mut search = ChildSearch { frame_pid, found: 0 };
    unsafe { EnumChildWindows(hwnd, Some(enum_child), &mut search as *mut _ as LPARAM) };
    (search.found != 0).then_some(search.found)
}
