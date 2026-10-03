//! JSONLの書き出しと日付ローテーション。
//!
//! レコードごとに、その時刻（区間は開始時刻、イベントは発生時刻）の日付のファイルへ1行追記してflushする。
//! 新しいファイル（空のファイル）に書くときは先にヘッダーを書く。

use std::fs::{File, OpenOptions};
use std::io::Write;

use chrono::NaiveDate;

use crate::common::model::{Header, Record, SCHEMA};
use crate::common::{clock, console, errlog, paths};

pub struct Writer {
    cur: Option<(NaiveDate, File)>,
    /// `run`（前面実行）のときは標準出力にも出す
    echo: bool,
}

impl Writer {
    pub fn new(echo: bool) -> Self {
        if let Err(e) = std::fs::create_dir_all(paths::log_dir()) {
            errlog::log(&format!("create log dir: {e}"));
        }
        Writer { cur: None, echo }
    }

    pub fn write_all(&mut self, recs: &[Record]) {
        for r in recs {
            self.write(r);
        }
    }

    pub fn write(&mut self, rec: &Record) {
        let line = rec.to_json();
        if self.echo {
            console::out(&line);
        }
        let date = rec.time().unwrap_or_else(clock::now).date_naive();
        if let Err(e) = self.append(date, &line) {
            errlog::log(&format!("write log ({date}): {e}"));
            self.cur = None;
        }
    }

    fn append(&mut self, date: NaiveDate, line: &str) -> std::io::Result<()> {
        let f = self.file_for(date)?;
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        f.flush()
    }

    fn file_for(&mut self, date: NaiveDate) -> std::io::Result<&mut File> {
        if !matches!(&self.cur, Some((d, _)) if *d == date) {
            let path = paths::log_file(date);
            let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
            if f.metadata()?.len() == 0 {
                let header = Record::Header(Header {
                    schema: SCHEMA,
                    app: env!("CARGO_PKG_VERSION").to_string(),
                    host: paths::hostname(),
                    user: paths::username(),
                    tz: clock::tz_offset(),
                });
                let line = header.to_json();
                if self.echo {
                    console::out(&line);
                }
                f.write_all(line.as_bytes())?;
                f.write_all(b"\n")?;
                f.flush()?;
            }
            self.cur = Some((date, f));
        }
        Ok(&mut self.cur.as_mut().expect("file just opened").1)
    }
}
