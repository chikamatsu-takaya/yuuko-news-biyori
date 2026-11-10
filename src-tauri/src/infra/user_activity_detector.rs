//! 利用者がこのパソコンを操作中か（最後のキーボード・マウス入力からの経過）と、物理メモリの空きを
//! OS に問い合わせる（ローカルLLMの自動要約を「作業中でメモリに余裕がないとき」だけ待たせるため）。
//!
//! 判定（何分で「操作中」とみなすか・空きが何 GB 未満なら待つか）は `auto_summary_queue` 側の純粋関数に置き、
//! ここは OS の値をそのまま返すだけにする（判定を Windows なしでテストできるよう、OS 依存部分はこの trait の
//! 実装だけに閉じ込める。`fullscreen_detector` / `meeting_detector` と同じ形）。
//! 問い合わせは自動要約の記事を取り出す前と、その確かめ直し（60 秒ごと）のときだけ行う。フック・常駐スレッドは持たない。
//!
//! 画面ロック中は別途判定しない。ロック中はこのセッションへの入力が無いので、最後の入力からの経過がそのまま延び、
//! 「操作中」の閾値（数分）を過ぎた時点で「操作していない」と同じ扱いになるため（詳細設計書 §5.4.5 の自動要約キュー）。
//! Windows 以外では値を取れないため None を返し、呼び出し側は「待たせない」側に倒す。

use std::fmt::Debug;
use std::time::Duration;

/// 操作中かどうか・メモリの空きの問い合わせの境界。テストでは固定値を返す実装を差し込む。
pub trait UserActivityDetector: Debug + Send + Sync {
    /// 最後のキーボード・マウス入力からの経過時間。取れないときは None。
    fn idle_time(&self) -> Option<Duration>;
    /// 物理メモリの空き（新しく使い始められる量。OS の「利用可能」）のバイト数。取れないときは None。
    fn available_memory_bytes(&self) -> Option<u64>;
}

/// 実行環境の OS に問い合わせる実装。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemUserActivityDetector;

impl UserActivityDetector for SystemUserActivityDetector {
    #[cfg(windows)]
    fn idle_time(&self) -> Option<Duration> {
        windows_impl::idle_time()
    }

    /// Windows 以外は判定手段を持たないため、取れない（呼び出し側は待たせない）。
    #[cfg(not(windows))]
    fn idle_time(&self) -> Option<Duration> {
        None
    }

    #[cfg(windows)]
    fn available_memory_bytes(&self) -> Option<u64> {
        windows_impl::available_memory_bytes()
    }

    #[cfg(not(windows))]
    fn available_memory_bytes(&self) -> Option<u64> {
        None
    }
}

/// 最後の入力時刻と今の時刻（どちらも起動からのミリ秒・32bit で約 49.7 日ごとに一周する）から経過を求める。
/// 一周をまたいでも正しく引けるよう wrapping_sub で引く（Windows の GetLastInputInfo / GetTickCount の値）。
/// 差が半周（`u32::MAX / 2`）を超えるのは、2つの値を読む間に入力があって最後の入力時刻のほうがわずかに先に
/// なったときなので、0（たった今の入力＝操作中）として扱う（約 24.8 日の経過と取り違えないため）。
#[cfg_attr(not(windows), allow(dead_code))]
fn elapsed_since_tick(now_tick: u32, last_input_tick: u32) -> Duration {
    let elapsed = now_tick.wrapping_sub(last_input_tick);
    if elapsed > u32::MAX / 2 {
        return Duration::ZERO;
    }
    Duration::from_millis(u64::from(elapsed))
}

#[cfg(windows)]
mod windows_impl {
    use std::time::Duration;

    use windows_sys::Win32::System::SystemInformation::{
        GetTickCount, GlobalMemoryStatusEx, MEMORYSTATUSEX,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    /// GetLastInputInfo（このセッションの最後の入力時刻）と GetTickCount（今の時刻）から経過を求める。
    /// 失敗（戻り値 0）は None とし、呼び出し側は待たせない（fail-open）。
    pub(super) fn idle_time() -> Option<Duration> {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: cbSize を設定した有効な LASTINPUTINFO へのポインタを渡すだけで、所有権の受け渡しは無い。
        let ok = unsafe { GetLastInputInfo(&mut info) };
        if ok == 0 {
            return None;
        }
        // SAFETY: 引数の無い時刻取得。
        let now = unsafe { GetTickCount() };
        Some(super::elapsed_since_tick(now, info.dwTime))
    }

    /// GlobalMemoryStatusEx の ullAvailPhys（物理メモリの利用可能量）を返す。失敗（戻り値 0）は None。
    pub(super) fn available_memory_bytes() -> Option<u64> {
        // SAFETY: MEMORYSTATUSEX は整数だけの構造体で、0 埋めは有効な値。
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: dwLength を設定した有効な MEMORYSTATUSEX へのポインタを渡すだけで、所有権の受け渡しは無い。
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        if ok == 0 {
            return None;
        }
        Some(status.ullAvailPhys)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 実機の OS から値が取れること（値そのものは環境で変わるので、取れることと範囲だけを見る）。
        /// 入力の経過は対話セッションでないと取れない環境（CI のサービス実行など）があるため、呼べることだけを見る。
        #[test]
        fn system_queries_return_values_on_windows() {
            let _ = idle_time();
            let available = available_memory_bytes().expect("memory status");
            assert!(available > 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_is_measured_across_the_tick_wrap_around() {
        assert_eq!(elapsed_since_tick(5_000, 2_000), Duration::from_secs(3));
        assert_eq!(elapsed_since_tick(1_000, 1_000), Duration::ZERO);
        // 49.7 日で一周した直後でも、一周前の入力からの経過として正しく引ける。
        assert_eq!(
            elapsed_since_tick(500, u32::MAX - 499),
            Duration::from_millis(1_000)
        );
    }

    #[test]
    fn last_input_slightly_after_now_is_treated_as_just_now() {
        // 今の時刻を読んだ直後に入力があり、最後の入力時刻が 5 ミリ秒先になった。
        assert_eq!(elapsed_since_tick(1_000, 1_005), Duration::ZERO);
        assert_eq!(elapsed_since_tick(0, 1), Duration::ZERO);
        // 半周ちょうどまでは経過として扱い、それを超えたら 0。
        assert_eq!(
            elapsed_since_tick(u32::MAX / 2, 0),
            Duration::from_millis(u64::from(u32::MAX / 2))
        );
        assert_eq!(elapsed_since_tick(u32::MAX / 2 + 1, 0), Duration::ZERO);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_returns_nothing_so_the_caller_proceeds() {
        assert_eq!(SystemUserActivityDetector.idle_time(), None);
        assert_eq!(SystemUserActivityDetector.available_memory_bytes(), None);
    }
}
