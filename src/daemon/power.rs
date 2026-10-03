//! スリープの検出：時刻ジャンプ（主）、unbiased interrupt time、電源通知（補助）

use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;

use crate::common::clock::Time;

/// 1秒タイマーの間隔がこれ以上空いたらスリープとみなす
pub const JUMP_THRESHOLD_MS: i64 = 30_000;
/// スリープを含む時間と含まない時間の差がこれ以上増えたら記録する
const UNBIASED_THRESHOLD_MS: i64 = 1_000;

pub struct Power {
    last_tick: Time,
    last_bias_ms: Option<i64>,
    pub suspended_at: Option<Time>,
}

impl Power {
    pub fn new(now: Time) -> Self {
        Power { last_tick: now, last_bias_ms: sleep_bias_ms(), suspended_at: None }
    }

    /// 前回のタイマー時刻から30秒以上空いていれば `(前回のタイマー時刻, 経過ms)`
    pub fn check_jump(&mut self, now: Time) -> Option<(Time, i64)> {
        let prev = self.last_tick;
        self.last_tick = now;
        let gap = (now - prev).num_milliseconds();
        (gap >= JUMP_THRESHOLD_MS).then_some((prev, gap))
    }

    /// `GetTickCount64`（スリープを含む）と `QueryUnbiasedInterruptTime`（含まない）の差の増加量
    pub fn check_unbiased(&mut self) -> Option<i64> {
        let cur = sleep_bias_ms()?;
        let prev = self.last_bias_ms.replace(cur)?;
        let inc = cur - prev;
        (inc >= UNBIASED_THRESHOLD_MS).then_some(inc)
    }

    /// 電源通知による復帰。スリープしていた長さ（ms）を返す
    pub fn resumed(&mut self, now: Time) -> Option<i64> {
        self.suspended_at.take().map(|sa| (now - sa).num_milliseconds())
    }
}

fn sleep_bias_ms() -> Option<i64> {
    let mut unbiased: u64 = 0;
    unsafe {
        if QueryUnbiasedInterruptTime(&mut unbiased) == 0 {
            return None;
        }
        let tick = GetTickCount64();
        Some(tick as i64 - (unbiased / 10_000) as i64)
    }
}

