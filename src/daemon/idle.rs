//! GetLastInputInfo による最終入力からの経過時間

use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

/// 最終入力からの経過ミリ秒。取得に失敗したら `None`。
pub fn idle_ms() -> Option<u32> {
    let mut lii = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if unsafe { GetLastInputInfo(&mut lii) } == 0 {
        return None;
    }
    // 32bitミリ秒カウンタは約49.7日で一周するため、必ず wrapping_sub
    Some(unsafe { GetTickCount() }.wrapping_sub(lii.dwTime))
}
