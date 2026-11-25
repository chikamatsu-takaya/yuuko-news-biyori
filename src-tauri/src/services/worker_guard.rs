//! 常駐の作業スレッド（自動要約キュー・ニュース取得とアーカイブ保守のスケジューラ）の 1 回分を、
//! panic から切り離して実行する小さな道具。
//!
//! なぜ必要か: 作業スレッドは起動時に 1 本だけ作り、ループで回り続ける。1 回分の処理のどこかで panic すると
//! スレッドごと終わり、アプリを再起動するまで自動要約・定期取得・アーカイブ保守が二度と動かなかった
//! （自動要約は「処理中」の表示のまま残った）。1 回分を `catch_unwind` で包み、panic したらログに残して
//! 呼び出し側へ知らせ、呼び出し側が状態を戻してループを続けられるようにする。
//!
//! ログには場所の名前（固定の文字列）だけを出し、panic のペイロードは出さない。ペイロードは記事の本文・
//! URL などの外部由来の文字列を含みうるため（発生箇所のファイル:行は `install_panic_location_hook` が別に残す）。

use std::panic::{catch_unwind, AssertUnwindSafe};

/// `step` を panic から切り離して実行する。panic したら ERROR ログを 1 行残して `None` を返す。
///
/// `AssertUnwindSafe` を使うのは、包む処理が共有の状態（`Mutex` など）を触るため。共有の状態は
/// 呼び出し側で「panic の後も使える形」に戻す（poison は `into_inner` で外す・処理中の印を下ろす）前提とする。
pub(crate) fn run_isolated<T>(label: &'static str, step: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(step)) {
        Ok(value) => Some(value),
        Err(_) => {
            log::error!("{label} panicked; recovered and the worker keeps running");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_value_when_the_step_does_not_panic() {
        assert_eq!(run_isolated("test step", || 42), Some(42));
    }

    #[test]
    fn panicking_step_returns_none_and_the_next_step_still_runs() {
        let mut outcomes = Vec::new();
        for attempt in 1..=3 {
            outcomes.push(run_isolated("test step", move || {
                if attempt == 1 {
                    panic!("first attempt fails");
                }
                attempt
            }));
        }
        assert_eq!(outcomes, vec![None, Some(2), Some(3)]);
    }
}
