//! マイク使用状況（会議検知用）。CapabilityAccessManager のレジストリを読む。
//!
//! - パッケージアプリ：ルート直下のサブキー（例：`MSTeams_8wekyb3d8bbwe`）
//! - 従来型アプリ：`NonPackaged` 以下のサブキー（exeパスの `\` が `#` に置換されたキー名）
//! - 使用中：`LastUsedTimeStart != 0` かつ `LastUsedTimeStop == 0`

use std::collections::BTreeSet;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    RRF_RT_REG_QWORD,
};

const ROOT: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
const NON_PACKAGED: &str = "NonPackaged";

pub struct Mic {
    active: BTreeSet<String>,
}

pub struct Diff {
    pub on: Vec<String>,
    pub off: Vec<String>,
}

impl Mic {
    pub fn new() -> Self {
        Mic { active: BTreeSet::new() }
    }

    /// 前回からの差分。レジストリが読めなければ `None`（状態は変えない）
    pub fn poll(&mut self) -> Option<Diff> {
        let now = scan()?;
        let on = now.difference(&self.active).cloned().collect();
        let off = self.active.difference(&now).cloned().collect();
        self.active = now;
        Some(Diff { on, off })
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Key(HKEY);

impl Key {
    fn open(parent: HKEY, path: &str) -> Option<Key> {
        let mut h: HKEY = std::ptr::null_mut();
        let p = wide(path);
        let r = unsafe { RegOpenKeyExW(parent, p.as_ptr(), 0, KEY_READ, &mut h) };
        (r == ERROR_SUCCESS).then_some(Key(h))
    }

    fn subkeys(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = [0u16; 512];
        for i in 0.. {
            let mut len = buf.len() as u32;
            let r = unsafe {
                RegEnumKeyExW(self.0, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            };
            if r != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }

    fn qword(&self, subkey: &str, value: &str) -> u64 {
        let sk = wide(subkey);
        let v = wide(value);
        let mut data: u64 = 0;
        let mut size = std::mem::size_of::<u64>() as u32;
        let r = unsafe {
            RegGetValueW(self.0, sk.as_ptr(), v.as_ptr(), RRF_RT_REG_QWORD, std::ptr::null_mut(), &mut data as *mut u64 as *mut _, &mut size)
        };
        if r == ERROR_SUCCESS {
            data
        } else {
            0
        }
    }

    fn in_use(&self, subkey: &str) -> bool {
        self.qword(subkey, "LastUsedTimeStart") != 0 && self.qword(subkey, "LastUsedTimeStop") == 0
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn scan() -> Option<BTreeSet<String>> {
    let root = Key::open(HKEY_CURRENT_USER, ROOT)?;
    let mut active = BTreeSet::new();
    for name in root.subkeys() {
        if name.eq_ignore_ascii_case(NON_PACKAGED) {
            continue;
        }
        if root.in_use(&name) {
            active.insert(name);
        }
    }
    if let Some(np) = Key::open(root.0, NON_PACKAGED) {
        for name in np.subkeys() {
            if np.in_use(&name) {
                active.insert(name);
            }
        }
    }
    Some(active)
}
