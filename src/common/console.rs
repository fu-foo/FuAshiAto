//! 標準出力への書き出し。
//!
//! コンソールに接続されていれば `WriteConsoleW` で書く。パイプやファイルにリダイレクトされている場合は
//! コンソールの出力コードページ（日本語環境なら通常932）に変換して書く。PowerShell 5.1 はネイティブコマンドの
//! 出力をこのコードページで解釈するため、`$(fuashiato status --brief)` でも文字化けしない。

use std::io::Write;

#[derive(Clone, Copy)]
pub enum Stream {
    Out,
    Err,
}

pub fn out(s: &str) {
    write_line(Stream::Out, s);
}

pub fn err(s: &str) {
    write_line(Stream::Err, s);
}

#[cfg(windows)]
fn write_line(stream: Stream, s: &str) {
    use windows_sys::Win32::Globalization::{WideCharToMultiByte, CP_UTF8};
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetConsoleOutputCP, GetStdHandle, WriteConsoleW, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE,
    };

    let line = format!("{s}\r\n");
    let wide: Vec<u16> = line.encode_utf16().collect();
    unsafe {
        let h = GetStdHandle(match stream {
            Stream::Out => STD_OUTPUT_HANDLE,
            Stream::Err => STD_ERROR_HANDLE,
        });
        if h.is_null() || h == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return;
        }
        let mut mode = 0;
        if GetConsoleMode(h, &mut mode) != 0 {
            let mut written = 0;
            WriteConsoleW(h, wide.as_ptr(), wide.len() as u32, &mut written, std::ptr::null());
            return;
        }
        // リダイレクト先：コンソールの出力コードページで書く
        let cp = match GetConsoleOutputCP() {
            0 => CP_UTF8,
            cp => cp,
        };
        let bytes = if cp == CP_UTF8 {
            line.into_bytes()
        } else {
            let n = WideCharToMultiByte(cp, 0, wide.as_ptr(), wide.len() as i32, std::ptr::null_mut(), 0, std::ptr::null(), std::ptr::null_mut());
            if n <= 0 {
                line.into_bytes()
            } else {
                let mut buf = vec![0u8; n as usize];
                WideCharToMultiByte(cp, 0, wide.as_ptr(), wide.len() as i32, buf.as_mut_ptr(), n, std::ptr::null(), std::ptr::null_mut());
                buf
            }
        };
        let _ = match stream {
            Stream::Out => std::io::stdout().lock().write_all(&bytes),
            Stream::Err => std::io::stderr().lock().write_all(&bytes),
        };
    }
}

#[cfg(not(windows))]
fn write_line(stream: Stream, s: &str) {
    let _ = match stream {
        Stream::Out => writeln!(std::io::stdout(), "{s}"),
        Stream::Err => writeln!(std::io::stderr(), "{s}"),
    };
}
