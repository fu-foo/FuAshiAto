//! スタートアップ登録：`FOLDERID_Startup\fuashiato.lnk`（`HKCU\...\Run` は使わない）
//!
//! windows-sys は COM インターフェースのメソッドを持たないため、必要な vtable を自前で定義する。

use std::ffi::c_void;
use std::path::PathBuf;

use windows_sys::core::{BOOL, GUID, HRESULT, PCWSTR, PWSTR};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::UI::Shell::{FOLDERID_Startup, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE;

use crate::common::{console, paths};
use crate::daemon::wide;

const LINK_NAME: &str = "fuashiato.lnk";

const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

type Unused = usize;

#[repr(C)]
#[allow(dead_code)]
struct IShellLinkWVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: Unused,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_path: Unused,
    get_id_list: Unused,
    set_id_list: Unused,
    get_description: Unused,
    set_description: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_working_directory: Unused,
    set_working_directory: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_arguments: Unused,
    set_arguments: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_hotkey: Unused,
    set_hotkey: Unused,
    get_show_cmd: Unused,
    set_show_cmd: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    get_icon_location: Unused,
    set_icon_location: Unused,
    set_relative_path: Unused,
    resolve: Unused,
    set_path: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
}

#[repr(C)]
#[allow(dead_code)]
struct IPersistFileVtbl {
    query_interface: Unused,
    add_ref: Unused,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_class_id: Unused,
    is_dirty: Unused,
    load: Unused,
    save: unsafe extern "system" fn(*mut c_void, PCWSTR, BOOL) -> HRESULT,
    save_completed: Unused,
    get_cur_file: Unused,
}

unsafe fn vtbl<T>(obj: *mut c_void) -> &'static T {
    &**(obj as *mut *const T)
}

fn startup_dir() -> Result<PathBuf, String> {
    unsafe {
        let mut p: PWSTR = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(&FOLDERID_Startup, KF_FLAG_DEFAULT as u32, std::ptr::null_mut(), &mut p);
        if hr < 0 || p.is_null() {
            return Err(format!("SHGetKnownFolderPath failed (0x{hr:08X})"));
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p as *const c_void);
        Ok(PathBuf::from(s))
    }
}

fn check(hr: HRESULT, what: &str) -> Result<(), String> {
    if hr < 0 {
        Err(format!("{what} failed (0x{hr:08X})"))
    } else {
        Ok(())
    }
}

fn create_link(link: &PathBuf, target: &PathBuf, args: &str) -> Result<(), String> {
    unsafe {
        let hr = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        check(hr, "CoInitializeEx")?;
        let result = (|| {
            let mut sl: *mut c_void = std::ptr::null_mut();
            check(
                CoCreateInstance(&CLSID_SHELL_LINK, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ISHELL_LINK_W, &mut sl),
                "CoCreateInstance(ShellLink)",
            )?;
            let v: &IShellLinkWVtbl = vtbl(sl);
            let target_w = wide(&target.to_string_lossy());
            let workdir_w = wide(&target.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
            let args_w = wide(args);
            let desc_w = wide("fuashiato: 作業の記録を開始");
            let r = (|| {
                check((v.set_path)(sl, target_w.as_ptr()), "SetPath")?;
                check((v.set_arguments)(sl, args_w.as_ptr()), "SetArguments")?;
                check((v.set_working_directory)(sl, workdir_w.as_ptr()), "SetWorkingDirectory")?;
                check((v.set_description)(sl, desc_w.as_ptr()), "SetDescription")?;
                check((v.set_show_cmd)(sl, SW_SHOWMINNOACTIVE), "SetShowCmd")?;
                let mut pf: *mut c_void = std::ptr::null_mut();
                check((v.query_interface)(sl, &IID_IPERSIST_FILE, &mut pf), "QueryInterface(IPersistFile)")?;
                let pv: &IPersistFileVtbl = vtbl(pf);
                let link_w = wide(&link.to_string_lossy());
                let hr = (pv.save)(pf, link_w.as_ptr(), 1);
                (pv.release)(pf);
                check(hr, "IPersistFile::Save")
            })();
            (v.release)(sl);
            r
        })();
        CoUninitialize();
        result
    }
}

pub fn startup(arg: Option<&str>, explicit_dir: Option<&std::path::Path>) -> i32 {
    let dir = match startup_dir() {
        Ok(d) => d,
        Err(e) => {
            console::err(&format!("スタートアップフォルダを取得できませんでした: {e}"));
            return 10;
        }
    };
    let link = dir.join(LINK_NAME);
    match arg {
        Some("on") => {
            let exe = match std::env::current_exe() {
                Ok(p) => p,
                Err(e) => {
                    console::err(&format!("実行ファイルのパスを取得できませんでした: {e}"));
                    return 10;
                }
            };
            // --dir を指定したときだけショートカットにも入れる（既定は exe と同じフォルダなので不要）
            let args = match explicit_dir {
                Some(_) => format!("start --dir {}", super::quote_arg(&paths::log_dir().to_string_lossy())),
                None => "start".to_string(),
            };
            match create_link(&link, &exe, &args) {
                Ok(()) => {
                    console::out(&format!("スタートアップに登録しました: {}", link.display()));
                    console::out(&format!("  実行内容: {} {args}", exe.display()));
                    0
                }
                Err(e) => {
                    console::err(&format!("ショートカットを作成できませんでした: {e}"));
                    10
                }
            }
        }
        Some("off") => {
            if !link.exists() {
                console::out("スタートアップには登録されていません");
                return 0;
            }
            match std::fs::remove_file(&link) {
                Ok(()) => {
                    console::out(&format!("スタートアップから削除しました: {}", link.display()));
                    0
                }
                Err(e) => {
                    console::err(&format!("ショートカットを削除できませんでした: {e}"));
                    10
                }
            }
        }
        _ => {
            console::err("使い方: fuashiato startup on|off");
            3
        }
    }
}
