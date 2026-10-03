pub mod control;
pub mod start;
pub mod startup;
pub mod status;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

use crate::daemon::{wide, CLASS_NAME, TITLE_PREFIX};

/// 常駐プロセスのウィンドウとPID
pub fn find_daemon() -> Option<(HWND, u32)> {
    let class = wide(CLASS_NAME);
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    Some((hwnd, pid))
}

/// 常駐プロセスの出力先（隠しウィンドウのタイトルから読む）
pub fn daemon_dir() -> Option<std::path::PathBuf> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let (hwnd, _) = find_daemon()?;
    let mut buf = [0u16; 1024];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return None;
    }
    let title = String::from_utf16_lossy(&buf[..n as usize]);
    let dir = title.strip_prefix(TITLE_PREFIX)?;
    (!dir.is_empty()).then(|| std::path::PathBuf::from(dir))
}

/// コマンドライン引数用に引用符で囲む（末尾の `\` が `"` をエスケープしないよう二重にする）
pub fn quote_arg(s: &str) -> String {
    if s.ends_with('\\') {
        format!("\"{s}\\\"")
    } else {
        format!("\"{s}\"")
    }
}

/// 条件が満たされるまで最大 `ms` ミリ秒待つ
pub fn wait_until(ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let step = std::time::Duration::from_millis(100);
    let mut waited = 0;
    loop {
        if cond() {
            return true;
        }
        if waited >= ms {
            return false;
        }
        std::thread::sleep(step);
        waited += 100;
    }
}
