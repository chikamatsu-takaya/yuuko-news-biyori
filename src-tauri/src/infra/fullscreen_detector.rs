//! 全画面アプリ・プレゼン中かどうかを OS に問い合わせる（ゆうこ通知の抑制判定・自動要約の一時停止用）。
//!
//! 設計書（ゆうこ登場・通知挙動 §5.2）の「フルスクリーン中」「発表・プレゼンモード」に対応する。
//! 判定結果には全画面の種類（ふつうの全画面・Direct3D 全画面・プレゼンモード）を持たせる。
//! ゆうこ通知はどの種類でも抑制し（従来どおり）、ローカルLLMの自動要約は Direct3D 全画面と
//! プレゼンモードのときだけ止める（判断台帳 D158）。ふつうの全画面（QUNS_BUSY）は別モニターの
//! 全画面ウィンドウ・動画の全画面表示などでも返り続け、自動要約がいつまでも始まらなかったため（実機テスト A3）。
//! 判定ロジック（猶予・保留）を Windows なしでテストできるよう、OS 依存部分はこの trait の
//! 実装だけに閉じ込める。Windows 以外では判定を無効化し、常に「抑制しない」を返す。

use std::fmt::Debug;

/// 全画面の種類（OS の通知受付状態の区別をそのまま持つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenKind {
    /// ふつうの全画面アプリ（QUNS_BUSY。別モニターの全画面ウィンドウ・動画の全画面表示などでも返る）。
    App,
    /// Direct3D の全画面（QUNS_RUNNING_D3D_FULL_SCREEN。主にゲーム）。
    Game,
    /// プレゼンテーションモード（QUNS_PRESENTATION_MODE。発表中）。
    Presentation,
}

/// OS から得た「ユーザーが通知を受けられる状態か」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenStatus {
    /// 全画面アプリ・Direct3D 全画面・プレゼンモード中（種類付き）。
    Busy(FullscreenKind),
    /// 通知を出してよい状態。
    Free,
    /// OS への問い合わせに失敗した。呼び出し側は抑制しない（fail-open）。
    Unknown,
}

impl FullscreenStatus {
    /// ゆうこ通知を抑制すべき全画面か。種類を問わず抑制する（設定「フルスクリーン時は通知を抑制する」）。
    pub fn suppresses_notifications(self) -> bool {
        matches!(self, Self::Busy(_))
    }

    /// ローカルLLMの自動要約を止める全画面か。Direct3D 全画面（ゲーム）とプレゼンモードだけ（D158）。
    /// ふつうの全画面（`FullscreenKind::App`）と問い合わせ失敗（`Unknown`）では止めない。
    pub fn pauses_auto_summary(self) -> bool {
        matches!(
            self,
            Self::Busy(FullscreenKind::Game | FullscreenKind::Presentation)
        )
    }
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
    use super::{FullscreenKind, FullscreenStatus};
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

    /// 全画面アプリ（QUNS_BUSY）・Direct3D 全画面・プレゼンモードを種類付きの Busy にする。
    /// 静音時間（QUNS_QUIET_TIME）などは抑制・一時停止の条件ではないため Free として扱う。
    pub(super) fn classify(state: QUERY_USER_NOTIFICATION_STATE) -> FullscreenStatus {
        match state {
            QUNS_BUSY => FullscreenStatus::Busy(FullscreenKind::App),
            QUNS_RUNNING_D3D_FULL_SCREEN => FullscreenStatus::Busy(FullscreenKind::Game),
            QUNS_PRESENTATION_MODE => FullscreenStatus::Busy(FullscreenKind::Presentation),
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
        fn classify_marks_fullscreen_and_presentation_as_busy_with_kind() {
            assert_eq!(
                classify(QUNS_BUSY),
                FullscreenStatus::Busy(FullscreenKind::App)
            );
            assert_eq!(
                classify(QUNS_RUNNING_D3D_FULL_SCREEN),
                FullscreenStatus::Busy(FullscreenKind::Game)
            );
            assert_eq!(
                classify(QUNS_PRESENTATION_MODE),
                FullscreenStatus::Busy(FullscreenKind::Presentation)
            );
        }

        /// D158: 通知はどの全画面でも抑制し、自動要約は D3D 全画面・プレゼンモードだけで止める。
        #[test]
        fn os_states_map_to_notification_and_auto_summary_decisions() {
            for state in [
                QUNS_BUSY,
                QUNS_RUNNING_D3D_FULL_SCREEN,
                QUNS_PRESENTATION_MODE,
            ] {
                assert!(classify(state).suppresses_notifications());
            }
            assert!(!classify(QUNS_BUSY).pauses_auto_summary());
            assert!(classify(QUNS_RUNNING_D3D_FULL_SCREEN).pauses_auto_summary());
            assert!(classify(QUNS_PRESENTATION_MODE).pauses_auto_summary());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 通知の抑制は従来どおり全画面の種類を問わない（D158 で変えない側）。
    #[test]
    fn every_fullscreen_kind_suppresses_notifications() {
        for kind in [
            FullscreenKind::App,
            FullscreenKind::Game,
            FullscreenKind::Presentation,
        ] {
            assert!(FullscreenStatus::Busy(kind).suppresses_notifications());
        }
        assert!(!FullscreenStatus::Free.suppresses_notifications());
        assert!(!FullscreenStatus::Unknown.suppresses_notifications());
    }

    /// 自動要約はゲーム（D3D 全画面）と発表（プレゼンモード）だけで止める（D158）。
    #[test]
    fn only_game_and_presentation_pause_auto_summary() {
        assert!(!FullscreenStatus::Busy(FullscreenKind::App).pauses_auto_summary());
        assert!(FullscreenStatus::Busy(FullscreenKind::Game).pauses_auto_summary());
        assert!(FullscreenStatus::Busy(FullscreenKind::Presentation).pauses_auto_summary());
        assert!(!FullscreenStatus::Free.pauses_auto_summary());
        assert!(!FullscreenStatus::Unknown.pauses_auto_summary());
    }
}
