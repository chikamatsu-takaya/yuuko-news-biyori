//! メインウィンドウが非表示・最小化の間だけ、ゆうこ通知の判定を低頻度で実行し、
//! 候補があればゆうこ用ウィンドウ（デスクトップ右下）へ表示用データを渡す。
//!
//! 方針（設計書 §4.4 / §6、2026-10-05 決定）:
//! - 判定と状態更新はアプリ内通知と同じ `YuukoService::request_yuuko_notification` を通す。
//!   日次上限・クールタイム・時間帯・紹介済み管理を二重実装せず、永続化されたゆうこ状態を唯一の正とする。
//! - メイン表示中はアプリ内通知（React 側スケジューラ）だけが候補を生成する。本スレッドは
//!   判定を行わず、ゆうこ用ウィンドウを隠すだけにして二重通知を防ぐ。
//! - 判定は保存済みの設定・状態・記事一覧の読み込みだけで、HTML取得や AI 呼び出しは行わない。
//! - ゆうこ用ウィンドウの生成は Windows の同期 command デッドロックを避けるため、本スレッドからだけ行う。

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::app_lifecycle::MAIN_WINDOW_LABEL;
use crate::domain::yuuko::{
    RequestYuukoNotificationResult, YuukoNotificationState, YuukoResidentState,
};
use crate::services::yuuko_service::YuukoService;
use crate::yuuko_window::{self, YUUKO_WINDOW_LABEL};

/// 判定間隔。アプリ内通知のスケジューラ（5分）と揃え、表示中/非表示中で通知の出やすさを変えない。
/// クールタイムが最短60分のため、これより細かく見ても通知機会はほぼ増えない。
const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// デスクトップ通知（非表示中の判定とゆうこ用ウィンドウ表示）を有効にするか。
///
/// ゆうこ用ウィンドウはまだ中身のない透明なプレースホルダで、Windows（WebView2）の透明ウィンドウは
/// 透明部分でもマウス入力を受けるため、表示すると右下の見えない領域が他アプリのクリックを奪い、
/// 閉じる手段もない。さらに見えない表示のために通知枠・紹介済みを消費してしまう。
/// そのため UI 実装タスク（0eC3Fhgo）で描画と閉じる導線が揃うまで false にしておき、そこで true にする。
const DESKTOP_NOTIFICATION_ENABLED: bool = false;

/// ゆうこ用ウィンドウへ通知データを渡すイベント名。lib/tauri/yuuko.ts と一致させる。
pub const YUUKO_DESKTOP_NOTIFICATION_EVENT: &str = "yuuko-desktop-notification";

/// ゆうこ用ウィンドウへ渡す最小限の表示用データ。
///
/// 本文・要約・URL は渡さない（小型ウィンドウでは不要で、外部由来文字列の露出を最小にするため）。
/// title / balloon_text は外部由来を含み得るため、UI 側は必ずテキストとして描画する。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YuukoDesktopNotification {
    pub article_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balloon_text: Option<String>,
}

/// 1回の判定周期で行うこと。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickAction {
    /// メイン表示中: アプリ内通知に任せ、デスクトップ側は隠すだけ。
    HideDesktop,
    /// メイン非表示・最小化中: 通知判定を実行する。
    RunJudgement,
    /// メインの状態が分からない（破棄済み・取得失敗）: 二重通知や終了妨害を避けるため何もしない。
    Skip,
}

/// デスクトップ通知が無効な間は、判定（通知枠の消費）もウィンドウ生成・表示も行わない。
/// メイン表示中の hide は既存ウィンドウを隠すだけ（無ければ何もしない）で無害なため残す。
fn gate_tick(enabled: bool, action: TickAction) -> TickAction {
    match action {
        TickAction::RunJudgement if !enabled => TickAction::Skip,
        other => other,
    }
}

/// メインウィンドウの (表示中か, 最小化中か) から、この周期の動作を決める。
/// 取得できなかった場合（None）は安全側として何もしない。
fn decide_tick(main_window: Option<(bool, bool)>) -> TickAction {
    match main_window {
        Some((true, false)) => TickAction::HideDesktop,
        Some(_) => TickAction::RunJudgement,
        None => TickAction::Skip,
    }
}

/// 判定結果から、デスクトップに出す表示用データを取り出す。
///
/// - 新規に候補を選んだ（notified）場合に加え、既に active なニュース通知が残っている
///   （already_active: 例えばアプリ内で表示中にウィンドウを隠した）場合も対象にする。
///   永続状態の active 通知を「見えている側の画面」で出すことで、表示先が切り替わっても
///   通知枠を再消費せず、同じ通知を1か所にだけ出す。
/// - それ以外（抑制・候補なし・報酬優先など）は表示しない。
fn desktop_notification_from(
    result: &RequestYuukoNotificationResult,
) -> Option<YuukoDesktopNotification> {
    if !result.notified && result.reason != "already_active" {
        return None;
    }
    desktop_notification_from_state(&result.state)
}

/// active なニュース通知状態だけを表示用データへ変換する。タイトルが無いものは出さない。
fn desktop_notification_from_state(
    state: &YuukoNotificationState,
) -> Option<YuukoDesktopNotification> {
    let is_active_news = matches!(
        state.state,
        YuukoResidentState::Appearing
            | YuukoResidentState::BalloonVisible
            | YuukoResidentState::PreviewVisible
    );
    if !is_active_news {
        return None;
    }
    let article = state.preview_article.as_ref()?;
    Some(YuukoDesktopNotification {
        article_id: state
            .current_article_id
            .clone()
            .unwrap_or_else(|| article.article_id.clone()),
        title: article.title.clone(),
        balloon_text: state.balloon_text.clone(),
    })
}

/// 判定スレッドを開始する。起動直後はメインが表示中のため、最初の判定は1周期待ってから行う。
pub fn start<R: Runtime>(app: AppHandle<R>, yuuko_service: YuukoService) {
    std::thread::spawn(move || loop {
        std::thread::sleep(CHECK_INTERVAL);
        tick(&app, &yuuko_service);
    });
}

fn tick<R: Runtime>(app: &AppHandle<R>, yuuko_service: &YuukoService) {
    match gate_tick(
        DESKTOP_NOTIFICATION_ENABLED,
        decide_tick(main_window_visibility(app)),
    ) {
        TickAction::Skip => {}
        TickAction::HideDesktop => {
            // ウィンドウイベントでの非表示に失敗した場合の取りこぼし対策も兼ねる。
            if let Err(error) = yuuko_window::hide_yuuko_window(app) {
                log::warn!("メイン表示中にゆうこ用ウィンドウを隠せませんでした: {error}");
            }
        }
        TickAction::RunJudgement => run_judgement(app, yuuko_service),
    }
}

fn run_judgement<R: Runtime>(app: &AppHandle<R>, yuuko_service: &YuukoService) {
    let result = match yuuko_service.request_yuuko_notification() {
        Ok(result) => result,
        Err(error) => {
            log::warn!("非表示中のゆうこ通知判定に失敗しました。次回に再試行します: {error}");
            return;
        }
    };
    let Some(notification) = desktop_notification_from(&result) else {
        return;
    };

    // 判定中にメインが再表示された場合は、アプリ内の再表示（保存済み active の拾い直し）に任せる。
    if decide_tick(main_window_visibility(app)) != TickAction::RunJudgement {
        return;
    }
    // 既に出している active 通知を周期ごとに出し直すと、登場演出が繰り返されてしつこくなるため避ける。
    if !result.notified && is_yuuko_window_visible(app) {
        return;
    }

    if let Err(error) = present(app, &notification) {
        // 通知状態は永続化済みのため、メイン再表示時にアプリ内で拾い直せる（取りこぼさない）。
        log::warn!("ゆうこ用ウィンドウに通知を表示できませんでした: {error}");
    }
}

/// ゆうこ用ウィンドウへデータを渡してから表示する。
///
/// 初回生成直後はページがまだ購読していないためイベントが届かない。その場合に備え、
/// ページはマウント時に既存の get_yuuko_notification_state で現在の active 通知を取得する契約とする。
fn present<R: Runtime>(
    app: &AppHandle<R>,
    notification: &YuukoDesktopNotification,
) -> tauri::Result<()> {
    yuuko_window::ensure_yuuko_window(app)?;
    // メインウィンドウへは送らず、ゆうこ用ウィンドウだけに届ける。
    app.emit_to(
        YUUKO_WINDOW_LABEL,
        YUUKO_DESKTOP_NOTIFICATION_EVENT,
        notification,
    )?;
    yuuko_window::show_yuuko_window(app)
}

fn main_window_visibility<R: Runtime>(app: &AppHandle<R>) -> Option<(bool, bool)> {
    let window = app.get_webview_window(MAIN_WINDOW_LABEL)?;
    match (window.is_visible(), window.is_minimized()) {
        (Ok(visible), Ok(minimized)) => Some((visible, minimized)),
        (Err(error), _) | (_, Err(error)) => {
            log::warn!("メインウィンドウの表示状態を取得できませんでした: {error}");
            None
        }
    }
}

fn is_yuuko_window_visible<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(YUUKO_WINDOW_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::article::{ArticleReadState, ArticleSummaryDto};
    use crate::domain::yuuko::YuukoPositionMode;

    fn article(id: &str, title: &str) -> ArticleSummaryDto {
        ArticleSummaryDto {
            article_id: id.to_string(),
            title: title.to_string(),
            source_name: "source".to_string(),
            published_at_text: "today".to_string(),
            genre: "tech".to_string(),
            summary: Some("要約は渡さない".to_string()),
            is_favorite: false,
            read_state: ArticleReadState::Unread,
            recommendation_score: 0.9,
        }
    }

    fn state(resident: YuukoResidentState, with_article: bool) -> YuukoNotificationState {
        YuukoNotificationState {
            state: resident,
            position_mode: YuukoPositionMode::RightBottom,
            balloon_text: Some("気になるニュースがあるよ".to_string()),
            preview_article: with_article.then(|| article("article-001", "タイトル")),
            has_notification: false,
            current_article_id: with_article.then(|| "article-001".to_string()),
            reward_notification: None,
        }
    }

    fn result(
        notified: bool,
        reason: &str,
        state: YuukoNotificationState,
    ) -> RequestYuukoNotificationResult {
        RequestYuukoNotificationResult {
            notified,
            reason: reason.to_string(),
            state,
        }
    }

    #[test]
    fn main_visible_only_hides_desktop_without_judgement() {
        assert_eq!(decide_tick(Some((true, false))), TickAction::HideDesktop);
    }

    #[test]
    fn main_hidden_or_minimized_runs_judgement() {
        assert_eq!(decide_tick(Some((false, false))), TickAction::RunJudgement);
        assert_eq!(decide_tick(Some((true, true))), TickAction::RunJudgement);
        assert_eq!(decide_tick(Some((false, true))), TickAction::RunJudgement);
    }

    #[test]
    fn disabled_gate_never_judges_or_shows_desktop() {
        for main_window in [
            Some((false, false)),
            Some((true, true)),
            Some((false, true)),
            Some((true, false)),
            None,
        ] {
            let action = gate_tick(false, decide_tick(main_window));
            // 判定（通知枠の消費）とウィンドウ生成・表示につながる RunJudgement にはならない。
            assert_ne!(action, TickAction::RunJudgement, "main={main_window:?}");
        }
        // 有効時は従来どおり判定する。
        assert_eq!(
            gate_tick(true, decide_tick(Some((false, false)))),
            TickAction::RunJudgement
        );
    }

    #[test]
    fn unknown_main_window_state_does_nothing() {
        assert_eq!(decide_tick(None), TickAction::Skip);
    }

    #[test]
    fn newly_notified_candidate_becomes_minimal_payload() {
        let payload = desktop_notification_from(&result(
            true,
            "notified",
            state(YuukoResidentState::BalloonVisible, true),
        ));
        assert_eq!(
            payload,
            Some(YuukoDesktopNotification {
                article_id: "article-001".to_string(),
                title: "タイトル".to_string(),
                balloon_text: Some("気になるニュースがあるよ".to_string()),
            })
        );
    }

    #[test]
    fn existing_active_notification_is_moved_to_desktop() {
        let payload = desktop_notification_from(&result(
            false,
            "already_active",
            state(YuukoResidentState::PreviewVisible, true),
        ));
        assert_eq!(
            payload.map(|p| p.article_id),
            Some("article-001".to_string())
        );
    }

    #[test]
    fn suppressed_results_do_not_show_desktop() {
        for reason in [
            "disabled",
            "reward_pending",
            "daily_limit",
            "cooling_down",
            "outside_time_range",
            "no_candidate",
        ] {
            // 古い active 状態が返っても、抑制理由なら表示しない。
            let payload = desktop_notification_from(&result(
                false,
                reason,
                state(YuukoResidentState::BalloonVisible, true),
            ));
            assert_eq!(payload, None, "reason={reason}");
        }
    }

    #[test]
    fn non_active_or_title_less_state_is_not_shown() {
        assert_eq!(
            desktop_notification_from(&result(
                false,
                "already_active",
                state(YuukoResidentState::Waiting, true),
            )),
            None
        );
        assert_eq!(
            desktop_notification_from(&result(
                true,
                "notified",
                state(YuukoResidentState::BalloonVisible, false),
            )),
            None
        );
    }

    #[test]
    fn payload_serializes_with_camel_case_and_no_body_fields() {
        let json = serde_json::to_value(YuukoDesktopNotification {
            article_id: "a".to_string(),
            title: "t".to_string(),
            balloon_text: None,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "articleId": "a", "title": "t" }));
    }
}
