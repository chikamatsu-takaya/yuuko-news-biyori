//! 全画面アプリ・プレゼン中かどうかを OS に問い合わせる（ゆうこ通知の抑制判定用）。
//!
//! 設計書（ゆうこ登場・通知挙動 §5.2）の「フルスクリーン中」「発表・プレゼンモード」に対応する。
//! 判定ロジック（猶予・保留）を Windows なしでテストできるよう、OS 依存部分はこの trait の
//! 実装だけに閉じ込める。Windows 以外では判定を無効化し、常に「抑制しない」を返す。

use std::fmt::Debug;

/// OS から得た「ユーザーが通知を受けられる状態か」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenStatus {
    /// 全画面アプリ・Direct3D 全画面・プレゼンモード中（通知を抑制する）。
    Busy,
    /// 通知を出してよい状態。
    Free,
    /// OS への問い合わせに失敗した。呼び出し側は抑制しない（fail-open）。
    Unknown,
}

/// 全画面判定の境界。テストでは固定値を返す実装を差し込む。
pub trait FullscreenDetector: Debug + Send + Sync {
    fn detect(&self) -> FullscreenStatus;
}

/// 実行環境の OS に問い合わせる実装。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemFullscreenDetector;

impl FullscreenDetector for SystemFullscreenDetector {
    #[cfg(windows)]
    fn detect(&self) -> FullscreenStatus {
        windows_impl::query()
    }

    /// Windows 以外は判定手段を持たないため、抑制しない側（通知可）を返す。
    #[cfg(not(windows))]
    fn detect(&self) -> FullscreenStatus {
        FullscreenStatus::Free
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::FullscreenStatus;
    use windows_sys::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_BUSY,
        QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };

    /// SHQueryUserNotificationState で現在の通知受付状態を取得する。
    /// 失敗（HRESULT が負）は Unknown とし、詳細は呼び出し側でもログに出さない。
    pub(super) fn query() -> FullscreenStatus {
        let mut state: QUERY_USER_NOTIFICATION_STATE = 0;
        // SAFETY: 出力先として有効な i32 へのポインタを渡すだけで、所有権の受け渡しは無い。
        let hr = unsafe { SHQueryUserNotificationState(&mut state) };
        if hr < 0 {
            return FullscreenStatus::Unknown;
        }
        classify(state)
    }

    /// 全画面アプリ（QUNS_BUSY）・Direct3D 全画面・プレゼンモードだけを抑制対象にする。
    /// 静音時間（QUNS_QUIET_TIME）などは本タスクの抑制条件ではないため通知可として扱う。
    pub(super) fn classify(state: QUERY_USER_NOTIFICATION_STATE) -> FullscreenStatus {
        match state {
            QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE => {
                FullscreenStatus::Busy
            }
            _ => FullscreenStatus::Free,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::UI::Shell::{
            QUNS_ACCEPTS_NOTIFICATIONS, QUNS_APP, QUNS_NOT_PRESENT, QUNS_QUIET_TIME,
        };

        #[test]
        fn classify_marks_fullscreen_and_presentation_as_busy() {
            assert_eq!(classify(QUNS_BUSY), FullscreenStatus::Busy);
            assert_eq!(
                classify(QUNS_RUNNING_D3D_FULL_SCREEN),
                FullscreenStatus::Busy
            );
            assert_eq!(classify(QUNS_PRESENTATION_MODE), FullscreenStatus::Busy);
        }

        #[test]
        fn classify_treats_other_states_as_free() {
            for state in [
                QUNS_ACCEPTS_NOTIFICATIONS,
                QUNS_NOT_PRESENT,
                QUNS_QUIET_TIME,
                QUNS_APP,
            ] {
                assert_eq!(classify(state), FullscreenStatus::Free);
            }
        }
    }
}
