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
use crate::yuuko_window::{self, YuukoWindowStage, YUUKO_WINDOW_LABEL};

/// 判定間隔。アプリ内通知のスケジューラ（5分）と揃え、表示中/非表示中で通知の出やすさを変えない。
/// クールタイムが最短60分のため、これより細かく見ても通知機会はほぼ増えない。
const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// 全画面・プレゼン中に抑制した後の再確認間隔。5分間隔のままだと解除の検知が最大5分遅れ、
/// 設計書 §5.4 の「解除後 30〜180 秒の猶予」より大幅に遅れるため、抑制中に限って短くする。
/// 1回の判定は小さな JSON の読み込みと OS への問い合わせ1回だけで、抑制が終われば通常間隔へ戻る。
const FULLSCREEN_RECHECK_INTERVAL: Duration = Duration::from_secs(60);

/// デスクトップ通知（非表示中の判定とゆうこ用ウィンドウ表示）を有効にするか。
///
/// ゆうこ用ウィンドウの画面（吹き出し・2段階クリック・閉じる・自動退場）と、通知終了時に Rust 側で
/// ウィンドウを隠す導線が揃ったため有効にしている。透明部分のクリック奪取は、ウィンドウを
/// 表示段階ごとに中身ぎりぎりの大きさにすることで最小化している（yuuko_window::YuukoWindowStage）。
/// 実機で問題が出た場合に、判定（通知枠の消費）ごと止められる切り替え口として残す。
const DESKTOP_NOTIFICATION_ENABLED: bool = true;

/// ゆうこ用ウィンドウへ通知データを渡すイベント名。lib/tauri/yuuko.ts と一致させる。
pub const YUUKO_DESKTOP_NOTIFICATION_EVENT: &str = "yuuko-desktop-notification";
/// ゆうこ用ウィンドウで「詳しく見る」が確定したとき、メインウィンドウへ記事を開かせるイベント名。
/// lib/tauri/yuuko.ts の YUUKO_OPEN_ARTICLE_EVENT と一致させる。
pub const YUUKO_OPEN_ARTICLE_EVENT: &str = "yuuko-open-article";

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
    /// 既に軽量プレビュー段階か。ページの表示段階とウィンドウの大きさを Rust の状態に合わせるために使う。
    pub preview_visible: bool,
}

impl YuukoDesktopNotification {
    fn stage(&self) -> YuukoWindowStage {
        if self.preview_visible {
            YuukoWindowStage::Preview
        } else {
            YuukoWindowStage::Balloon
        }
    }
}

/// メインウィンドウへ渡す「記事を開く」要求。記事IDは永続状態の紹介中記事から決める。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct YuukoOpenArticleRequest {
    article_id: String,
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
        preview_visible: state.state == YuukoResidentState::PreviewVisible,
    })
}

/// 判定スレッドを開始する。起動直後はメインが表示中のため、最初の判定は1周期待ってから行う。
pub fn start<R: Runtime>(app: AppHandle<R>, yuuko_service: YuukoService) {
    std::thread::spawn(move || {
        let mut delay = CHECK_INTERVAL;
        loop {
            std::thread::sleep(delay);
            delay = tick(&app, &yuuko_service);
        }
    });
}

/// 1周期分を実行し、次の判定までの待ち時間を返す。
fn tick<R: Runtime>(app: &AppHandle<R>, yuuko_service: &YuukoService) -> Duration {
    match gate_tick(
        DESKTOP_NOTIFICATION_ENABLED,
        decide_tick(main_window_visibility(app)),
    ) {
        TickAction::Skip => CHECK_INTERVAL,
        TickAction::HideDesktop => {
            // ウィンドウイベントでの非表示に失敗した場合の取りこぼし対策も兼ねる。
            if let Err(error) = yuuko_window::hide_yuuko_window(app) {
                log::warn!("メイン表示中にゆうこ用ウィンドウを隠せませんでした: {error}");
            }
            CHECK_INTERVAL
        }
        TickAction::RunJudgement => run_judgement(app, yuuko_service),
    }
}

/// 判定結果の理由から次の判定までの待ち時間を決める。
///
/// - 全画面・プレゼン中: 解除をすぐ検知できるよう短い間隔で再確認する。
/// - 解除後の猶予中: 猶予が明けた直後に判定する（境界での取りこぼしを避けるため 1 秒足す）。
/// - それ以外: 通常間隔。
fn next_check_delay(reason: &str, grace_remaining: Option<Duration>) -> Duration {
    match reason {
        "fullscreen" => FULLSCREEN_RECHECK_INTERVAL,
        "fullscreen_grace" => grace_remaining
            .map(|remaining| (remaining + Duration::from_secs(1)).min(CHECK_INTERVAL))
            .unwrap_or(FULLSCREEN_RECHECK_INTERVAL),
        _ => CHECK_INTERVAL,
    }
}

fn run_judgement<R: Runtime>(app: &AppHandle<R>, yuuko_service: &YuukoService) -> Duration {
    let result = match yuuko_service.request_yuuko_notification() {
        Ok(result) => result,
        Err(error) => {
            log::warn!("非表示中のゆうこ通知判定に失敗しました。次回に再試行します: {error}");
            return CHECK_INTERVAL;
        }
    };
    let next_delay = next_check_delay(&result.reason, yuuko_service.fullscreen_grace_remaining());
    present_if_needed(app, &result);
    next_delay
}

/// 判定結果に表示すべき通知があれば、ゆうこ用ウィンドウに出す。
fn present_if_needed<R: Runtime>(app: &AppHandle<R>, result: &RequestYuukoNotificationResult) {
    let Some(notification) = desktop_notification_from(result) else {
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
    yuuko_window::show_yuuko_window(app, notification.stage())
}

/// ゆうこ通知を操作する既存 command（クリック確定 / 閉じる / 無視）の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YuukoAction {
    Click,
    Dismiss,
    Ignore,
}

/// command 処理後に、ゆうこ用ウィンドウとメインウィンドウへ行う後処理。
#[derive(Debug, Clone, PartialEq, Eq)]
enum FollowUp {
    None,
    /// 吹き出し→軽量プレビューへ進んだので、ウィンドウを中身に合わせて広げる。
    ResizeToPreview,
    /// 通知が終わった（閉じる / 無視 / 失敗）ので、ゆうこ用ウィンドウを隠す。
    Hide,
    /// 「詳しく見る」が確定したので、隠したうえでメインを前面表示して記事を開かせる。
    OpenInMain {
        article_id: String,
    },
}

/// command の呼び出し元・種類・結果から後処理を決める。
///
/// - ゆうこ用ウィンドウにはウィンドウ操作の権限が無いため、隠す・大きさ変更・メイン表示は
///   Rust で行う。新しい command や権限を増やさないよう、既存 command の後処理にしている。
/// - 結果が active なニュース通知でなくなったら、呼び出し元に関係なく隠す
///   （メイン表示中はもともと隠れているため無害で、取り残された透明ウィンドウを残さない）。
/// - ゆうこ用ウィンドウからの command が失敗した場合も、状態が不明なまま透明ウィンドウが
///   クリックを奪い続けないよう隠す（通知状態は永続化済みのため、メイン表示時にアプリ内で拾い直せる）。
fn decide_follow_up(
    from_yuuko_window: bool,
    action: YuukoAction,
    result: Option<&YuukoNotificationState>,
) -> FollowUp {
    let Some(state) = result else {
        return if from_yuuko_window {
            FollowUp::Hide
        } else {
            FollowUp::None
        };
    };
    match (from_yuuko_window, action, state.state) {
        // 2回目のクリック（PreviewVisible → Leaving）は「詳しく見る」の確定。
        (true, YuukoAction::Click, YuukoResidentState::Leaving) => {
            let article_id = state.current_article_id.clone().or_else(|| {
                state
                    .preview_article
                    .as_ref()
                    .map(|article| article.article_id.clone())
            });
            match article_id {
                Some(article_id) => FollowUp::OpenInMain { article_id },
                None => FollowUp::Hide,
            }
        }
        (true, YuukoAction::Click, YuukoResidentState::PreviewVisible) => FollowUp::ResizeToPreview,
        (_, _, resident) if is_active_news(resident) => FollowUp::None,
        _ => FollowUp::Hide,
    }
}

fn is_active_news(state: YuukoResidentState) -> bool {
    matches!(
        state,
        YuukoResidentState::Appearing
            | YuukoResidentState::BalloonVisible
            | YuukoResidentState::PreviewVisible
    )
}

/// ゆうこ通知 command の処理後に呼び、ゆうこ用ウィンドウの表示とメインの前面表示を状態に合わせる。
///
/// 後処理の失敗で command の結果（保存済みの状態）を変えないよう、失敗はログだけにする。
/// command（async・非メインスレッド）から呼ぶため、ウィンドウの生成は行わない。
pub fn after_yuuko_action<R: Runtime, E>(
    app: &AppHandle<R>,
    caller_label: &str,
    action: YuukoAction,
    result: &Result<YuukoNotificationState, E>,
) {
    let follow_up = decide_follow_up(
        caller_label == YUUKO_WINDOW_LABEL,
        action,
        result.as_ref().ok(),
    );
    match follow_up {
        FollowUp::None => {}
        FollowUp::ResizeToPreview => {
            if let Err(error) = yuuko_window::resize_yuuko_window(app, YuukoWindowStage::Preview) {
                log::warn!("ゆうこ用ウィンドウを軽量プレビューの大きさにできませんでした: {error}");
            }
        }
        FollowUp::Hide => hide_after_action(app),
        FollowUp::OpenInMain { article_id } => {
            hide_after_action(app);
            // 先に記事を開かせてから前面表示し、直前の画面が一瞬見えるのを避ける。
            // メインは非表示でも WebView とイベント購読は生きている。
            if let Err(error) = app.emit_to(
                MAIN_WINDOW_LABEL,
                YUUKO_OPEN_ARTICLE_EVENT,
                YuukoOpenArticleRequest { article_id },
            ) {
                log::warn!("メインウィンドウへ記事を開く要求を送れませんでした: {error}");
            }
            crate::app_lifecycle::show_main_window(app);
        }
    }
}

fn hide_after_action<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = yuuko_window::hide_yuuko_window(app) {
        log::warn!("通知終了に合わせてゆうこ用ウィンドウを隠せませんでした: {error}");
    }
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
    fn fullscreen_suppression_rechecks_sooner_than_normal_interval() {
        assert_eq!(
            next_check_delay("fullscreen", None),
            FULLSCREEN_RECHECK_INTERVAL
        );
        assert!(FULLSCREEN_RECHECK_INTERVAL < CHECK_INTERVAL);
    }

    #[test]
    fn grace_period_rechecks_right_after_grace_ends() {
        assert_eq!(
            next_check_delay("fullscreen_grace", Some(Duration::from_secs(95))),
            Duration::from_secs(96)
        );
        // 猶予の残りが取れない場合も通常間隔まで待たない。
        assert_eq!(
            next_check_delay("fullscreen_grace", None),
            FULLSCREEN_RECHECK_INTERVAL
        );
    }

    #[test]
    fn other_reasons_use_normal_interval() {
        for reason in ["notified", "cooling_down", "daily_limit", "no_candidate"] {
            assert_eq!(next_check_delay(reason, None), CHECK_INTERVAL);
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
    fn desktop_notification_is_enabled() {
        // UI 実装（0eC3Fhgo）で描画・閉じる導線・終了時の非表示が揃ったため有効にしている。
        // 非表示中は判定まで進む（ゲートで止まらない）ことで確認する。
        assert_eq!(
            gate_tick(
                DESKTOP_NOTIFICATION_ENABLED,
                decide_tick(Some((false, false)))
            ),
            TickAction::RunJudgement
        );
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
                preview_visible: false,
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
        let payload = payload.expect("active notification should be shown");
        assert_eq!(payload.article_id, "article-001");
        // 軽量プレビュー段階から再開し、ウィンドウもプレビューの大きさで出す。
        assert!(payload.preview_visible);
        assert_eq!(payload.stage(), YuukoWindowStage::Preview);
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
            preview_visible: false,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "articleId": "a", "title": "t", "previewVisible": false })
        );
    }

    #[test]
    fn first_click_from_yuuko_window_resizes_to_preview() {
        let state = state(YuukoResidentState::PreviewVisible, true);
        assert_eq!(
            decide_follow_up(true, YuukoAction::Click, Some(&state)),
            FollowUp::ResizeToPreview
        );
        // メインのアプリ内通知のクリックでは、ゆうこ用ウィンドウに触れない。
        assert_eq!(
            decide_follow_up(false, YuukoAction::Click, Some(&state)),
            FollowUp::None
        );
    }

    #[test]
    fn confirm_click_from_yuuko_window_opens_article_in_main() {
        let state = state(YuukoResidentState::Leaving, true);
        assert_eq!(
            decide_follow_up(true, YuukoAction::Click, Some(&state)),
            FollowUp::OpenInMain {
                article_id: "article-001".to_string()
            }
        );
        // メイン側の「詳しく見る」はメイン自身が遷移するため、ゆうこ用ウィンドウを隠すだけ。
        assert_eq!(
            decide_follow_up(false, YuukoAction::Click, Some(&state)),
            FollowUp::Hide
        );
        // 紹介記事が分からない確定は、メインを開かず隠すだけにする。
        let without_article = YuukoNotificationState {
            preview_article: None,
            current_article_id: None,
            ..state
        };
        assert_eq!(
            decide_follow_up(true, YuukoAction::Click, Some(&without_article)),
            FollowUp::Hide
        );
    }

    #[test]
    fn dismiss_and_ignore_hide_yuuko_window() {
        let waiting = state(YuukoResidentState::Waiting, false);
        for action in [YuukoAction::Dismiss, YuukoAction::Ignore] {
            for from_yuuko_window in [true, false] {
                assert_eq!(
                    decide_follow_up(from_yuuko_window, action, Some(&waiting)),
                    FollowUp::Hide,
                    "action={action:?} from_yuuko={from_yuuko_window}"
                );
            }
        }
        // 報酬通知が優先された（非active）場合も隠す。
        let reward = state(YuukoResidentState::RewardNotifying, true);
        assert_eq!(
            decide_follow_up(true, YuukoAction::Dismiss, Some(&reward)),
            FollowUp::Hide
        );
    }

    #[test]
    fn failed_command_from_yuuko_window_hides_it() {
        for action in [
            YuukoAction::Click,
            YuukoAction::Dismiss,
            YuukoAction::Ignore,
        ] {
            assert_eq!(decide_follow_up(true, action, None), FollowUp::Hide);
            // メイン側の失敗はアプリ内通知の責務のため、ここでは何もしない。
            assert_eq!(decide_follow_up(false, action, None), FollowUp::None);
        }
    }

    #[test]
    fn still_active_balloon_keeps_window() {
        let balloon = state(YuukoResidentState::BalloonVisible, true);
        assert_eq!(
            decide_follow_up(true, YuukoAction::Dismiss, Some(&balloon)),
            FollowUp::None
        );
    }
}
