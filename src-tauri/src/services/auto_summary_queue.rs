//! ニュース取得後の自動要約キュー。
//!
//! 方針（判断台帳 D55）:
//! - 未要約の記事をおすすめ順に並べ、専用の低頻度スレッド1本で**1件ずつ**要約する。
//!   要約・再説明・感想の生成と保存は既存の `SummaryService::generate_article_summary`
//!   （出力検証・原子的保存込み）をそのまま使う。
//! - キューの状態はメモリだけに持つ。完了は記事ファイルの `status.summarized` で表し、
//!   新しい状態ファイルや記事ファイルの保存形式は増やさない。失敗回数は再起動で消え、
//!   起動時に残っている未要約記事をもう一度並べ直す。
//! - 失敗した記事は他の記事の後ろへ回して再試行し、合計 `MAX_ATTEMPTS` 回失敗したら
//!   再起動まで `Failed` のままにする（他の記事の処理は止めない）。
//! - 設定 `ai.autoSummaryEnabled` が有効なときだけ動く（新規インストールの既定は有効・判断台帳 D103。
//!   既存の設定ファイルで欄が無いときは従来どおり無効として読む）。
//! - 設定の AI プロバイダが実装済みの実AI（Gemini・同梱ローカルLLM）以外のときは動かない（判断台帳 D56 / D103）。
//!   固定応答で記事を「要約済み」にしてしまうと、後で実AIを使えるようになっても自動では作り直されないため。
//! - 実AIを呼べる見込みが無いときも動かさない（投入・取り出しの両方で確かめる）。Gemini はキーの有無
//!   （値は読まない・ログに出さない）、ローカルは同梱物の有無と大きさだけを見る（起動も SHA-256 の照合もしない）。
//!   呼べないまま動かすと、保存されない出力のために失敗と再試行を繰り返すだけになるため。
//! - 実AIの失敗・利用枠超過で Mock の代替出力になった場合、および実AIの出力が検証に落ちた場合は保存せず
//!   失敗として扱い、再試行→失敗の流れに乗せる（`generate_article_summary_without_fallback`、D56 / D104）。
//! - 1回の投入はおすすめ順の上位 `maxDailyRecommendations` 件まで（常駐負荷を抑えるため、D56。ローカルでも同じ）。
//! - ローカルLLMはこのパソコンの CPU とメモリを使うため、全画面（プレゼン・ゲーム）中とマイク使用中
//!   （会議中を含む）は取り出しを止めて待つ（D103）。止めた記事は失敗にも試行回数にも数えず、待機列の先頭に残す。
//!   判定はゆうこ通知と同じ OS 問い合わせ（`fullscreen_detector` / `meeting_detector`）を使い、通知の抑制設定には
//!   左右されない（通知を出すかどうかと、PC に負荷をかけてよいかは別の話のため）。
//! - ローカルLLMは要約の要求が来たときにだけ起動し、最後の利用から5分で止まる（`local_llm_service`）。
//!   キューは1件ずつ短い間隔で処理するため、まとめて要約する間はモデルを読み込んだまま使う。
//!   待機列が空になったら `LocalLlmService::release_soon` を呼び、利用者の操作が直近に無ければ
//!   5分を待たずに約60秒で止めてもらう（自動要約の後にメモリ約2GBを長く握らないため）。
//!   自動要約からの要求は `for_background` の窓口を通し、利用者の操作とは別に数える（summary_service）。
//!   要約済みの記事は AI を呼ばずに外すので、要約するものが無いときは起動しない。
//! - 終了時は新しい記事を取り出さない。処理中の1件は待たずに終了してよい。記事の保存は
//!   一時ファイルへ書き切ってから差し替える方式（article_repository の atomic_write）のため、
//!   書きかけの .md は残らない（差し替えの細部と隙間の扱いは article_repository 側の説明に従う）。
//! - 設定ファイルが読めないとき（保存中の差し替え・一時的な読み取りエラー）は、無効とは扱わず
//!   待機列を残したまま取り出しを見送り、確かめ直す（`SettingsUnavailable`）。
//!   読めない一瞬のために待機列を捨てると、次の投入（ニュース取得後など）まで自動要約が止まるため。
//!   読めない状態が続くときは、確かめ直す間隔を 60 秒から倍々に延ばして最長 10 分にする
//!   （`settings_retry_interval`）。待機列は捨てない（捨てても次の投入でまた並ぶだけで、読めるように
//!   なったときにすぐ再開できる方が分かりやすいため。メモリ上の ID の列なので保持の負荷は小さい）。
//!   警告は「読めなくなったとき」に1回だけ、読めるようになったら info を1回だけ出す。
//! - 取り出した記事は AI を呼ぶ前に記事ファイルで要約済みか確かめ直し、手動要約や並べ直しとの
//!   競合で要約済みになった記事を上書きしない。AI 呼び出し中に手動要約された場合も、保存の直前に
//!   同じ書き込みロック内で確かめ直して上書きせず、失敗ではなく完了として外す。
//! - 実AIの出力が1つでも Mock に切り替わったら、その記事の残りの AI 呼び出しはしない
//!   （キー未設定・利用枠超過で、保存しない出力のために外部AIを呼び続けないため）。
//! - 上限まで失敗した記事は投入件数の上限に数えない（失敗した記事で枠が埋まり、他の記事が
//!   自動要約されなくなるのを防ぐため）。
//! - AI 呼び出し中はキューのロックを持たない（画面からの状態取得を待たせない）。
//! - 手動の要約（「今すぐ要約」など）と重ねない。手動で生成中の記事は取り出さずに外し（`Skipped`）、
//!   自動で生成中の記事も、段の合間に手動の要約が始まったら残りの段をやめる（`YieldedToManual`。
//!   summary_service）。どちらも失敗に数えない。手動の要約が失敗した記事は、次の投入でまた並ぶ。
//! - ローカルLLMは `-np 1` で要求を1つずつ処理するため、利用者の要求（手動の要約・用語解説・フレーズ解説・
//!   接続テスト）の最中と、その終わりから `MANUAL_PRIORITY_GRACE`（45 秒）の間は次の記事を取り出さない。
//!   また、llama-server の起動失敗が続いて起動を控えている間（`START_FAILURE_COOLDOWN`）も取り出さない
//!   （取り出すと失敗の試行回数だけが進み、記事が `Failed` になるため）。どちらも `LocalLlmService::background_wait`
//!   で判定し、止めた記事は待機列の先頭に残す。待ちは `LOCAL_LLM_RECHECK_MAX` 以下の区切りで測り直す
//!   （設定の切り替えにも追いつくため。常時ポーリングはしない）。Gemini の設定では見ない。
//! - 動かさないと決めたとき（理由付き）・止めたとき・待つとき・動き出したとき・空になったとき・投入した件数を
//!   INFO ログへ1行で残す（`QueueNotice`。投入以外は直前と同じなら繰り返さない。本文・URL・キーは出さない）。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use crate::domain::article::{
    ArticleDetailDto, ArticleHistoryItemDto, ArticleSummaryDto, SummaryState,
};
use crate::domain::settings::{AiProvider, PersistedSettings};
use crate::domain::summary::GenerateArticleSummaryParams;
use crate::error::AppError;
use crate::infra::fullscreen_detector::{
    FullscreenDetector, FullscreenStatus, SystemFullscreenDetector,
};
use crate::infra::meeting_detector::{MeetingDetector, SystemMeetingDetector};
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::article_service::ArticleService;
use crate::services::local_llm_service::LocalLlmService;
use crate::services::summary_service::{AutoSummaryOutcome, SummaryService};

/// 1記事あたりの試行回数の上限（初回を含めた合計）。
const MAX_ATTEMPTS: u32 = 3;

/// 起動後、最初の1件を処理するまでの待ち時間。起動時のニュース取得・画面表示と競合させず、
/// OS 自動起動の直後でネットワークがまだ使えない時間帯に AI を呼ばないため 60 秒待つ。
const STARTUP_DELAY: Duration = Duration::from_secs(60);

/// Gemini で1件処理するごとに空ける間隔。1件で AI を3回呼ぶため、Gemini の無料枠の毎分上限に
/// 近づかないよう、30秒空けて「1分あたり最大2件（AI呼び出し6回）」程度に抑える。
const GEMINI_ITEM_INTERVAL: Duration = Duration::from_secs(30);

/// ローカルLLMで1件処理するごとに空ける間隔。利用枠は無いが、1件の生成中は CPU を使い続けるため、
/// 10秒だけ空けて他の操作に CPU を返す。ローカルLLMの「5分使わなければ止める」より十分短いので、
/// まとめて要約する間にモデルを読み込み直すことはない。
const LOCAL_ITEM_INTERVAL: Duration = Duration::from_secs(10);

/// 全画面・マイク使用中で止めたとき、次に確かめるまでの間隔（OS への問い合わせを繰り返しすぎないため）。
const PAUSE_RECHECK_INTERVAL: Duration = Duration::from_secs(60);

/// ローカルLLMの都合（手動の要求の優先・起動を控えている間）で取り出しを待つときの、1回の待ちの上限。
/// 起動を控える 10 分を1回で待たず、設定の切り替え（Gemini への変更など）に追いつけるようにする。
const LOCAL_LLM_RECHECK_MAX: Duration = Duration::from_secs(60);

/// 設定が読めない状態が続くときの、確かめ直す間隔の上限（60 秒から倍々に延ばす）。
const SETTINGS_RETRY_MAX: Duration = Duration::from_secs(10 * 60);

/// 同じ待ちの下限。残りがごく短いときに細かく起き直さないため。
const LOCAL_LLM_RECHECK_MIN: Duration = Duration::from_secs(1);

/// 自動要約を動かしてよいかの判定結果。設定が読めないとき（`Unknown`）は、無効（待機列を捨てる）と分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enablement {
    Enabled,
    Disabled,
    /// 設定が読めず判定できない。待機列を残して見送る。
    Unknown,
}

impl From<bool> for Enablement {
    fn from(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

/// 1回の処理ステップの結果。ログとテストで使う。
#[derive(Debug, Clone, PartialEq, Eq)]
enum StepOutcome {
    /// 取り出す記事が無い、または処理中の記事がある（同時に2件は処理しない）。
    Idle,
    /// 終了要求済みのため取り出さなかった。
    Stopped,
    /// 自動要約が無効のため、待機中の記事を捨てて何もしなかった。
    Disabled,
    /// 設定が読めなかったため取り出さなかった（待機列は残し、失敗に数えない。後で確かめ直す）。
    SettingsUnavailable,
    /// 全画面・マイク使用中のため取り出さなかった（記事は待機列に残し、失敗に数えない）。
    Paused,
    Succeeded(String),
    /// 取り出した時点で既に要約済みだったため、AI を呼ばずに外した。
    Skipped(String),
    /// AI 呼び出し中に手動要約などで要約済みになったため、保存せずに外した（失敗に数えない）。
    AlreadySummarized(String),
    /// 生成の途中で同じ記事の手動要約が始まったため、残りの段をやめて外した（失敗に数えない）。
    YieldedToManual(String),
    /// 失敗したが上限前のため、キューの最後へ戻した。
    Retrying(String),
    /// 上限に達したため、再起動まで Failed のままにする。
    Failed(String),
}

#[derive(Debug, Default)]
struct QueueState {
    pending: VecDeque<String>,
    processing: Option<String>,
    failure_counts: HashMap<String, u32>,
    failed: HashSet<String>,
}

/// キューの状態と、ワーカーを起こすための合図。サービスに依存しないためテストで直接動かせる。
#[derive(Debug, Default)]
struct QueueCore {
    state: Mutex<QueueState>,
    signal: Condvar,
    stopped: AtomicBool,
}

impl QueueCore {
    /// 状態は単純な集合だけなので、他スレッドの panic で poison されても中身をそのまま使う。
    fn lock(&self) -> MutexGuard<'_, QueueState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 待機列をおすすめ順の最新の並びで置き換える。
    /// 処理中・上限到達の記事は入れない。既に要約済みになった記事は渡されないので自然に外れる。
    fn replace_pending(&self, ordered_ids: Vec<String>) {
        let mut state = self.lock();
        let pending = ordered_ids
            .into_iter()
            .filter(|id| state.processing.as_ref() != Some(id) && !state.failed.contains(id))
            .collect();
        state.pending = pending;
        drop(state);
        self.signal.notify_all();
    }

    /// 上限まで失敗した記事IDの写し。投入件数の上限から除くために使う。
    fn failed_ids(&self) -> HashSet<String> {
        self.lock().failed.clone()
    }

    fn clear_pending(&self) {
        self.lock().pending.clear();
    }

    /// 待機中の記事も処理中の記事も無いか（待機列が空になったか）。
    fn is_drained(&self) -> bool {
        let state = self.lock();
        state.pending.is_empty() && state.processing.is_none()
    }

    fn state_of(&self, article_id: &str) -> SummaryState {
        let state = self.lock();
        if state.processing.as_deref() == Some(article_id) {
            SummaryState::Processing
        } else if state.failed.contains(article_id) {
            SummaryState::Failed
        } else if state.pending.iter().any(|id| id == article_id) {
            SummaryState::Waiting
        } else {
            SummaryState::None
        }
    }

    /// 止める判定なしで次の1件を処理する（テスト用）。
    #[cfg(test)]
    fn process_next<E, R, D, S>(&self, is_enabled: E, is_done: D, summarize: S) -> StepOutcome
    where
        E: FnOnce() -> R,
        R: Into<Enablement>,
        D: FnOnce(&str) -> bool,
        S: FnOnce(&str) -> Result<AutoSummaryOutcome, AppError>,
    {
        self.process_next_unless_paused(is_enabled, || false, is_done, summarize)
    }

    /// 次の1件を処理する。`summarize` の実行中はロックを持たない。
    /// `is_paused` が true なら取り出さずに `Paused` を返す（記事は待機列の先頭に残り、失敗に数えない）。
    /// `is_paused` は待機中の記事があるときだけ呼ぶ（取り出すものが無いのに OS へ問い合わせないため）。
    /// `is_done` は記事ファイルで要約済みか・手動の要約が生成中かを副作用なしに確かめる（既読状態は進めない）。
    /// true なら AI を呼ばずに外す（`Skipped`）。
    /// `is_enabled` が `Unknown`（設定が読めない）なら、待機列を残して `SettingsUnavailable` を返す。
    fn process_next_unless_paused<E, R, P, D, S>(
        &self,
        is_enabled: E,
        is_paused: P,
        is_done: D,
        summarize: S,
    ) -> StepOutcome
    where
        E: FnOnce() -> R,
        R: Into<Enablement>,
        P: FnOnce() -> bool,
        D: FnOnce(&str) -> bool,
        S: FnOnce(&str) -> Result<AutoSummaryOutcome, AppError>,
    {
        if self.stopped.load(Ordering::SeqCst) {
            return StepOutcome::Stopped;
        }
        match is_enabled().into() {
            Enablement::Enabled => {}
            Enablement::Disabled => {
                self.clear_pending();
                return StepOutcome::Disabled;
            }
            Enablement::Unknown => return StepOutcome::SettingsUnavailable,
        }
        {
            let state = self.lock();
            if state.processing.is_some() || state.pending.is_empty() {
                return StepOutcome::Idle;
            }
        }
        if is_paused() {
            return StepOutcome::Paused;
        }

        let article_id = {
            let mut state = self.lock();
            if self.stopped.load(Ordering::SeqCst) {
                return StepOutcome::Stopped;
            }
            if state.processing.is_some() {
                return StepOutcome::Idle;
            }
            let Some(article_id) = state.pending.pop_front() else {
                return StepOutcome::Idle;
            };
            state.processing = Some(article_id.clone());
            article_id
        };

        if is_done(&article_id) {
            let mut state = self.lock();
            state.processing = None;
            state.failure_counts.remove(&article_id);
            return StepOutcome::Skipped(article_id);
        }

        let result = summarize(&article_id);

        let mut state = self.lock();
        state.processing = None;
        match result {
            Ok(AutoSummaryOutcome::Saved) => {
                state.failure_counts.remove(&article_id);
                StepOutcome::Succeeded(article_id)
            }
            // AI 呼び出し中に手動要約で要約済みになっていた（保存側で上書きを止めた）。
            // 失敗ではなく完了として外す。
            Ok(AutoSummaryOutcome::AlreadySummarized) => {
                state.failure_counts.remove(&article_id);
                StepOutcome::AlreadySummarized(article_id)
            }
            // 生成の途中で同じ記事の手動要約が始まった。手動の結果を待たずに外す（失敗に数えない）。
            Ok(AutoSummaryOutcome::YieldedToManual) => {
                state.failure_counts.remove(&article_id);
                StepOutcome::YieldedToManual(article_id)
            }
            Err(error) => {
                // 本文は出さず、記事IDとエラー種別だけを残す。
                log::warn!("auto summary failed for {article_id}: {error}");
                let count = state.failure_counts.entry(article_id.clone()).or_insert(0);
                *count += 1;
                if *count >= MAX_ATTEMPTS {
                    state.failed.insert(article_id.clone());
                    StepOutcome::Failed(article_id)
                } else {
                    state.pending.push_back(article_id.clone());
                    StepOutcome::Retrying(article_id)
                }
            }
        }
    }

    /// 待機中の記事が来るか終了要求が来るまで待つ。続けてよければ true。
    fn wait_for_work(&self) -> bool {
        let mut state = self.lock();
        loop {
            if self.stopped.load(Ordering::SeqCst) {
                return false;
            }
            if !state.pending.is_empty() && state.processing.is_none() {
                return true;
            }
            state = self
                .signal
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// 次の1件までの間隔を空ける。終了要求があればすぐ戻る。
    fn wait_interval(&self, interval: Duration) {
        let state = self.lock();
        let _ = self
            .signal
            .wait_timeout_while(state, interval, |_| !self.stopped.load(Ordering::SeqCst));
    }

    fn request_stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        // ロックを取ってから起こし、待機に入る直前の取りこぼしを防ぐ。
        let _state = self.lock();
        self.signal.notify_all();
    }
}

/// 実AIを呼べる見込みがあるかの判定（Gemini はキーの有無、ローカルは同梱物の有無）。AI は呼ばない。
type AiReadyCheck = Arc<dyn Fn(AiProvider) -> bool + Send + Sync>;

/// 全画面・マイク使用中（会議中を含む）かの判定。ローカルLLMの自動要約を止めるために使う。
type UserBusyCheck = Arc<dyn Fn() -> Option<BusyCause> + Send + Sync>;

/// 待機列が空になったときの知らせ。本番は `LocalLlmService::release_soon`（早めの停止の予約）。
type DrainedHook = Arc<dyn Fn() + Send + Sync>;

/// ローカルLLMの都合で取り出しを待つ長さ。本番は `LocalLlmService::background_wait`。
type LocalLlmWait = Arc<dyn Fn() -> Option<Duration> + Send + Sync>;

/// 全画面（プレゼン・ゲームなど）またはマイク使用中か。会議中は「会議アプリ起動中 かつ マイク使用中」
/// なので、マイク使用中を見れば含まれる。OS 判定に失敗したときは止めない（ゆうこ通知と同じ fail-open）。
/// どのアプリかは扱わず、どちらの理由かだけを返す（ログに理由を残すため）。
fn user_busy_cause_on_this_pc() -> Option<BusyCause> {
    if matches!(SystemFullscreenDetector.detect(), FullscreenStatus::Busy) {
        Some(BusyCause::Fullscreen)
    } else if SystemMeetingDetector.is_mic_in_use() {
        Some(BusyCause::MicInUse)
    } else {
        None
    }
}

/// ローカルLLMの自動要約を止める理由（全画面・マイク使用中）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusyCause {
    Fullscreen,
    MicInUse,
    /// テストで差し替えた判定（理由を区別しない）。
    #[cfg(test)]
    Test,
}

impl BusyCause {
    fn label(self) -> &'static str {
        match self {
            Self::Fullscreen => "fullscreen",
            Self::MicInUse => "mic_in_use",
            #[cfg(test)]
            Self::Test => "test",
        }
    }
}

/// 自動要約を動かさない理由（`is_auto_summary_allowed` の判定）。ログに1行で残すため分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OffReason {
    /// 設定 `ai.autoSummaryEnabled` が無効。
    DisabledInSettings,
    /// AI プロバイダが実AI（Gemini・ローカル）ではない（Mock・未実装の openai）。
    NotRealAi(AiProvider),
    /// 実AIを呼べる見込みが無い（Gemini はキー未設定、ローカルは同梱物が無い・壊れている）。
    AiNotReady(AiProvider),
}

impl OffReason {
    fn describe(self) -> String {
        match self {
            Self::DisabledInSettings => "reason=disabled_in_settings".to_string(),
            Self::NotRealAi(provider) => format!("reason=not_real_ai provider={provider:?}"),
            Self::AiNotReady(provider) => format!("reason=ai_not_ready provider={provider:?}"),
        }
    }
}

/// 自動要約の状態の知らせ（INFO ログ）。同じ知らせは続けて出さない（`AutoSummaryQueue::note`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueNotice {
    /// 動かさないと決めた（理由付き）。
    Off(OffReason),
    /// 全画面・マイク使用中のため取り出しを止めている。
    Paused(BusyCause),
    /// ローカルLLMの都合（手動の要求の優先・起動を控えている間）で取り出しを待っている。
    WaitingForLocalLlm,
    /// 記事を取り出して要約を始めた（止まっていた後の再開を含む）。
    Running,
    /// 待機列が空になった。
    Drained,
}

/// 自動要約キュー。AppState と NewsScheduler で共有する（clone しても同じキューを指す）。
#[derive(Clone)]
pub struct AutoSummaryQueue {
    core: Arc<QueueCore>,
    article_service: ArticleService,
    summary_service: SummaryService,
    settings_repository: SettingsRepository,
    /// 本番は `SummaryService::is_real_ai_ready`。テストでは環境変数・同梱物を用意せずに差し替える
    /// （並列テストで環境変数を書き換えると互いに干渉するため）。
    ai_ready: AiReadyCheck,
    /// 本番は `user_busy_cause_on_this_pc`。テストでは OS に依存しない判定へ差し替える。
    user_busy: UserBusyCheck,
    /// 待機列が空になったときに呼ぶ。既定は何もしない（`with_local_llm` で差し込む）。
    on_drained: DrainedHook,
    /// 取り出す前に待つ長さ（ローカルLLMの設定のときだけ見る）。既定は待たない（`with_local_llm` で差し込む）。
    local_llm_wait: LocalLlmWait,
    /// 設定が読めない状態か。警告を状態が変わったときだけ出すために持つ（clone しても同じ値を指す）。
    settings_unreadable: Arc<AtomicBool>,
    /// 最後に INFO ログへ出した状態の知らせ。同じ知らせを繰り返さないために持つ（clone しても同じ値を指す）。
    last_notice: Arc<Mutex<Option<QueueNotice>>>,
}

impl AutoSummaryQueue {
    pub fn new(
        article_service: ArticleService,
        summary_service: SummaryService,
        settings_repository: SettingsRepository,
    ) -> Self {
        let readiness = summary_service.clone();
        Self {
            core: Arc::new(QueueCore::default()),
            article_service,
            summary_service,
            settings_repository,
            ai_ready: Arc::new(move |provider| readiness.is_real_ai_ready(provider)),
            user_busy: Arc::new(user_busy_cause_on_this_pc),
            on_drained: Arc::new(|| {}),
            local_llm_wait: Arc::new(|| None),
            settings_unreadable: Arc::new(AtomicBool::new(false)),
            last_notice: Arc::new(Mutex::new(None)),
        }
    }

    /// 同梱ローカルLLMとつなぐ（lib.rs で、要約・用語解説に使うのと同じ窓口を渡す）。
    /// - 待機列が空になったら早めの停止を頼む。動いていなければ何もしない。直近の手動操作があるときに
    ///   いつ止めるか（手動操作から5分たつまでは止めない）は `LocalLlmService` が決める。
    /// - 次の記事を取り出す前に、手動の要求の最中・直後か、起動を控えている間かを尋ね、その間は待つ。
    pub fn with_local_llm(mut self, local_llm: LocalLlmService) -> Self {
        let release = local_llm.clone();
        self.on_drained = Arc::new(move || release.release_soon());
        self.local_llm_wait = Arc::new(move || local_llm.background_wait());
        self
    }

    /// ローカルLLMの都合で待つ長さの判定を差し替える（テスト用）。
    #[cfg(test)]
    fn with_local_llm_wait(
        mut self,
        wait: impl Fn() -> Option<Duration> + Send + Sync + 'static,
    ) -> Self {
        self.local_llm_wait = Arc::new(wait);
        self
    }

    /// 待機列が空になったときの知らせを差し替える（テスト用）。
    #[cfg(test)]
    fn with_drained_hook(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_drained = Arc::new(hook);
        self
    }

    /// 実AIを呼べる見込みの判定を差し替える（テスト用）。
    #[cfg(test)]
    fn with_ai_check(
        mut self,
        ai_ready: impl Fn(AiProvider) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.ai_ready = Arc::new(ai_ready);
        self
    }

    /// 全画面・マイク使用中の判定を差し替える（テスト用）。
    #[cfg(test)]
    fn with_busy_check(mut self, user_busy: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.user_busy = Arc::new(move || user_busy().then_some(BusyCause::Test));
        self
    }

    /// 未要約の記事をおすすめ順で待機列へ並べ直す。ニュース取得の完了後・起動時・設定保存後に呼ぶ。
    /// 無効時は待機列を空にする（処理中の1件はそのまま終わらせる）。設定が読めないときは
    /// 今の待機列を変えない（読めない一瞬のために捨てない）。
    /// 動かさないと決めたとき（理由付き）と、並べた件数を INFO ログへ1行で残す（記事の中身・URL は出さない）。
    pub fn enqueue_unsummarized(&self) {
        let Some(settings) = self.load_settings() else {
            return;
        };
        if !self.allowed(&settings) {
            self.core.clear_pending();
            return;
        }
        match self.article_service.list_unsummarized_article_ids() {
            Ok(ids) => {
                let unsummarized = ids.len();
                let failed = self.core.failed_ids();
                let capped = cap_for_enqueue(ids, &settings, &failed);
                let queued = capped.len();
                self.core.replace_pending(capped);
                log::info!(
                    "auto summary queued {queued} of {unsummarized} unsummarized article(s): provider={:?}",
                    settings.to_dto().ai_provider
                );
            }
            Err(error) => log::warn!("failed to list unsummarized articles: {error}"),
        }
    }

    /// 自動要約を動かしてよいか。動かさないときは理由を INFO ログへ残す（同じ理由は繰り返さない）。
    /// 動かしてよいときは「動かさない」の記録を外し、後で再び止まったときにもう一度残せるようにする。
    fn allowed(&self, settings: &PersistedSettings) -> bool {
        match auto_summary_off_reason(settings, |provider| (self.ai_ready)(provider)) {
            Some(reason) => {
                self.note(QueueNotice::Off(reason));
                false
            }
            None => {
                self.forget_notice(|notice| matches!(notice, QueueNotice::Off(_)));
                true
            }
        }
    }

    /// 状態の知らせを INFO ログへ1行で残す。直前と同じ知らせなら出さない（取り出しのたび・確かめ直すたびに
    /// 同じ行を繰り返さないため）。キー・本文・URL は含めない。出したら true。
    fn note(&self, notice: QueueNotice) -> bool {
        {
            let mut last = self
                .last_notice
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *last == Some(notice) {
                return false;
            }
            *last = Some(notice);
        }
        match notice {
            QueueNotice::Off(reason) => {
                log::info!("auto summary is not running: {}", reason.describe())
            }
            QueueNotice::Paused(cause) => log::info!(
                "auto summary paused while the pc is busy: cause={} (rechecking every {}s)",
                cause.label(),
                PAUSE_RECHECK_INTERVAL.as_secs()
            ),
            QueueNotice::WaitingForLocalLlm => log::info!(
                "auto summary is waiting for the local llm (a manual request or a start cooldown)"
            ),
            QueueNotice::Running => log::info!(
                "auto summary started: {} article(s) left in the queue",
                self.core.lock().pending.len()
            ),
            QueueNotice::Drained => log::info!("auto summary queue is empty"),
        }
        true
    }

    /// 直前の知らせが `matches` に当たれば忘れる（次に同じ知らせが来たらまた出す）。
    fn forget_notice(&self, matches: impl Fn(QueueNotice) -> bool) {
        let mut last = self
            .last_notice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.is_some_and(matches) {
            *last = None;
        }
    }

    /// 専用スレッドでワーカーを開始する。起動時に残っている未要約記事も対象にする。
    pub fn start(&self) {
        let queue = self.clone();
        log::info!("auto summary worker starts in {}s", STARTUP_DELAY.as_secs());
        std::thread::spawn(move || {
            // 終了要求があれば待ちを打ち切る（その後の wait_for_work が false を返して終わる）。
            queue.core.wait_interval(STARTUP_DELAY);
            queue.enqueue_unsummarized();
            queue.run_worker();
        });
    }

    fn run_worker(&self) {
        // 設定が続けて読めなかった回数（確かめ直す間隔を延ばすため）。
        let mut settings_unavailable_streak: u32 = 0;
        while self.core.wait_for_work() {
            // 手動の要求の最中・直後、起動を控えている間は取り出さない（記事は待機列の先頭に残る）。
            if let Some(wait) = self.local_llm_wait() {
                self.note(QueueNotice::WaitingForLocalLlm);
                log::debug!(
                    "auto summary is waiting {}s for the local llm",
                    wait.as_secs()
                );
                self.core.wait_interval(wait);
                continue;
            }
            let outcome = self.core.process_next_unless_paused(
                || self.is_enabled(),
                || self.is_paused(),
                |article_id| self.is_done_or_taken(article_id),
                |article_id| {
                    // AI を呼ぶ直前。止まっていた後（待ち・一時停止・空）から動き出したときだけ1行残す。
                    self.note(QueueNotice::Running);
                    self.summary_service
                        .generate_article_summary_without_fallback(GenerateArticleSummaryParams {
                            article_id: article_id.to_string(),
                        })
                },
            );
            self.notify_if_drained(&outcome);
            if outcome != StepOutcome::SettingsUnavailable {
                settings_unavailable_streak = 0;
            }
            match outcome {
                // AI を呼んだ後は、保存しなかった場合も含めて間隔を空ける
                // （Gemini は毎分上限を守るため、ローカルは CPU を他の操作へ返すため）。
                StepOutcome::Succeeded(_)
                | StepOutcome::AlreadySummarized(_)
                | StepOutcome::YieldedToManual(_)
                | StepOutcome::Retrying(_) => self.core.wait_interval(self.item_interval()),
                StepOutcome::Failed(article_id) => {
                    log::warn!("auto summary gave up after {MAX_ATTEMPTS} attempts: {article_id}");
                    self.core.wait_interval(self.item_interval())
                }
                StepOutcome::Paused => {
                    log::debug!("auto summary paused while the pc is busy");
                    self.core.wait_interval(PAUSE_RECHECK_INTERVAL)
                }
                // 設定が読めなかった。待機列は残し、少し待って確かめ直す。
                StepOutcome::SettingsUnavailable => {
                    settings_unavailable_streak = settings_unavailable_streak.saturating_add(1);
                    self.core
                        .wait_interval(settings_retry_interval(settings_unavailable_streak));
                }
                StepOutcome::Skipped(_)
                | StepOutcome::Idle
                | StepOutcome::Stopped
                | StepOutcome::Disabled => {}
            }
        }
    }

    /// 記事を取り出して片付けた（または無効で捨てた）結果、待機列が空になったら知らせる。
    /// 次の1件までの間隔を待つ前に呼び、ローカルLLMの早めの停止の猶予をすぐ数え始めてもらう。
    /// 後から記事が投入されれば、その要約の要求が予約を取り消す（`local_llm_service`）。
    fn notify_if_drained(&self, outcome: &StepOutcome) {
        if took_an_item(outcome) && self.core.is_drained() {
            // 無効で捨てた（Disabled）ときは、理由の行（Off）を残したままにする。
            if *outcome != StepOutcome::Disabled {
                self.note(QueueNotice::Drained);
            }
            (self.on_drained)();
        }
    }

    /// 次の1件までの間隔。設定のプロバイダで決める（読めないときは長い方＝Gemini 用の安全側）。
    fn item_interval(&self) -> Duration {
        let provider = self
            .load_settings()
            .map(|settings| settings.to_dto().ai_provider);
        item_interval_for(provider)
    }

    /// 次の記事を取り出す前に待つ長さ。ローカルLLMで要約する設定のときだけ尋ねる。
    fn local_llm_wait(&self) -> Option<Duration> {
        let provider = self
            .load_settings()
            .map(|settings| settings.to_dto().ai_provider);
        local_llm_wait_for(provider, || (self.local_llm_wait)())
    }

    /// ローカルLLMで要約する設定のときだけ、全画面・マイク使用中なら止める。
    /// Gemini は このパソコンの CPU をほとんど使わないため止めない。
    /// 止めたときは理由（全画面・マイク使用中）を INFO ログへ1回だけ残す。
    fn is_paused(&self) -> bool {
        let is_local = self
            .load_settings()
            .is_some_and(|settings| settings.to_dto().ai_provider == AiProvider::Local);
        if !is_local {
            return false;
        }
        match (self.user_busy)() {
            Some(cause) => {
                self.note(QueueNotice::Paused(cause));
                true
            }
            None => false,
        }
    }

    /// 終了要求。以後は新しい記事を取り出さない（処理中の1件は待たない）。
    pub fn request_stop(&self) {
        self.core.request_stop();
    }

    /// 記事一覧の DTO にキューの状態を反映する（保存済み＝Done はそのまま）。
    pub fn apply_to_summaries(&self, items: &mut [ArticleSummaryDto]) {
        for item in items {
            item.summary_state = self.resolve_state(&item.article_id, item.summary_state);
        }
    }

    /// 履歴の DTO にキューの状態を反映する（一覧と同じ規則）。
    pub fn apply_to_history_items(&self, items: &mut [ArticleHistoryItemDto]) {
        for item in items {
            item.summary_state = self.resolve_state(&item.article_id, item.summary_state);
        }
    }

    /// 記事詳細の DTO にキューの状態を反映する。
    pub fn apply_to_detail(&self, detail: &mut ArticleDetailDto) {
        detail.summary_state = self.resolve_state(&detail.article_id, detail.summary_state);
    }

    fn resolve_state(&self, article_id: &str, persisted: SummaryState) -> SummaryState {
        resolve_summary_state(persisted, self.core.state_of(article_id))
    }

    /// 取り出した記事を AI を呼ばずに外すか。手動の要約が生成中（同じ記事を二重に生成しないため）、
    /// または記事ファイルで要約済みなら true。
    fn is_done_or_taken(&self, article_id: &str) -> bool {
        if self
            .summary_service
            .is_manual_summary_in_progress(article_id)
        {
            log::info!("auto summary skipped {article_id}: a manual summary is in progress");
            return true;
        }
        self.is_summarized(article_id)
    }

    /// 記事ファイルで要約済みかを確かめる。読めないときは未要約として扱い、要約側の失敗として数える。
    fn is_summarized(&self, article_id: &str) -> bool {
        match self.article_service.is_article_summarized(article_id) {
            Ok(summarized) => summarized,
            Err(error) => {
                log::warn!("failed to check summary state for {article_id}: {error}");
                false
            }
        }
    }

    /// 設定が読めないときは取り出さない（外部AIの利用枠を守る安全側）。ただし無効とは扱わず
    /// 待機列を残す（`Enablement::Unknown`）。
    fn is_enabled(&self) -> Enablement {
        match self.load_settings() {
            Some(settings) => self.allowed(&settings).into(),
            None => Enablement::Unknown,
        }
    }

    /// 設定を読む。読めないときは固定の文言だけを残す（エラー文にパス等が入り得るため出さない）。
    /// 警告は読めなくなったときに1回だけ、読めるようになったら info を1回だけ出す（続く間は繰り返さない）。
    fn load_settings(&self) -> Option<PersistedSettings> {
        match self.settings_repository.load_or_default() {
            Ok(settings) => {
                if self.settings_unreadable.swap(false, Ordering::SeqCst) {
                    log::info!("auto summary: settings are readable again");
                }
                Some(settings)
            }
            Err(_) => {
                if !self.settings_unreadable.swap(true, Ordering::SeqCst) {
                    log::warn!("auto summary: settings could not be read; keeping the queue");
                }
                None
            }
        }
    }
}

/// 自動要約を動かしてよいか。設定が有効で、AI プロバイダが実装済みの実AI（Gemini・ローカル）で、
/// それを呼べる見込みがあるときだけ true（判断台帳 D56 / D103）。
/// openai は実AI呼び出しが未実装で常に Mock 応答になり、毎回失敗するだけなので
/// Mock と同じく動かさない。未知の値は DTO 変換で Mock 扱いになる。
/// `ai_ready` は Gemini ならキーの有無、ローカルなら同梱物の有無だけを返す安い確認で、
/// 設定で無効なとき・実AI以外のときは呼ばない（不要な確認をしないため）。
#[cfg(test)]
fn is_auto_summary_allowed(
    settings: &PersistedSettings,
    ai_ready: impl FnOnce(AiProvider) -> bool,
) -> bool {
    auto_summary_off_reason(settings, ai_ready).is_none()
}

/// 自動要約を動かさない理由（純粋関数）。動かしてよければ None。判定の規則は `is_auto_summary_allowed` のとおり。
/// 理由はプロバイダ名と固定の分類だけで、キーの値・同梱物の場所は扱わない。
fn auto_summary_off_reason(
    settings: &PersistedSettings,
    ai_ready: impl FnOnce(AiProvider) -> bool,
) -> Option<OffReason> {
    if !settings.ai.auto_summary_enabled {
        return Some(OffReason::DisabledInSettings);
    }
    let provider = settings.to_dto().ai_provider;
    if !matches!(provider, AiProvider::Gemini | AiProvider::Local) {
        return Some(OffReason::NotRealAi(provider));
    }
    if !ai_ready(provider) {
        return Some(OffReason::AiNotReady(provider));
    }
    None
}

/// 待機列から記事を取り出した（または無効で捨てた）結果か。取り出していない
/// （Idle・Stopped・Paused）ときは、空になった知らせを出さない（同じ知らせを繰り返さないため）。
fn took_an_item(outcome: &StepOutcome) -> bool {
    match outcome {
        StepOutcome::Succeeded(_)
        | StepOutcome::Skipped(_)
        | StepOutcome::AlreadySummarized(_)
        | StepOutcome::YieldedToManual(_)
        | StepOutcome::Retrying(_)
        | StepOutcome::Failed(_)
        | StepOutcome::Disabled => true,
        StepOutcome::Idle
        | StepOutcome::Stopped
        | StepOutcome::Paused
        | StepOutcome::SettingsUnavailable => false,
    }
}

/// ローカルLLMの都合で取り出しを待つ長さ（純粋関数）。ローカルLLMの設定のときだけ `wait` を尋ね、
/// 1回の待ちを `LOCAL_LLM_RECHECK_MIN`〜`LOCAL_LLM_RECHECK_MAX` に収める。
fn local_llm_wait_for(
    provider: Option<AiProvider>,
    wait: impl FnOnce() -> Option<Duration>,
) -> Option<Duration> {
    if provider != Some(AiProvider::Local) {
        return None;
    }
    wait().map(|remaining| remaining.clamp(LOCAL_LLM_RECHECK_MIN, LOCAL_LLM_RECHECK_MAX))
}

/// 設定が `streak` 回続けて読めなかったときに、次に確かめるまでの間隔（純粋関数）。
/// 60 秒から倍々に延ばし、`SETTINGS_RETRY_MAX`（10 分）で止める。
fn settings_retry_interval(streak: u32) -> Duration {
    let doublings = streak.saturating_sub(1).min(16);
    PAUSE_RECHECK_INTERVAL
        .saturating_mul(1u32 << doublings)
        .min(SETTINGS_RETRY_MAX)
}

/// 1件処理するごとに空ける間隔。ローカルは短く、それ以外（Gemini・設定を読めないとき）は長く空ける。
fn item_interval_for(provider: Option<AiProvider>) -> Duration {
    match provider {
        Some(AiProvider::Local) => LOCAL_ITEM_INTERVAL,
        _ => GEMINI_ITEM_INTERVAL,
    }
}

/// 1回の投入件数を、おすすめ順の上位 `maxDailyRecommendations` 件に絞る（常駐負荷を抑えるため）。
/// 上限まで失敗した記事（`failed`）は再起動まで並ばないため、先に除いてから数える
/// （除かないと失敗した記事が枠を埋め、その分ほかの記事が自動要約されなくなる）。
/// ローカルLLMでも同じ上限にする（判断台帳 D56 / D103。1件ずつ処理し、CPU を使い続けないため）。
fn cap_for_enqueue(
    ordered_ids: Vec<String>,
    settings: &PersistedSettings,
    failed: &HashSet<String>,
) -> Vec<String> {
    let limit = settings.news.max_daily_recommendations as usize;
    ordered_ids
        .into_iter()
        .filter(|id| !failed.contains(id))
        .take(limit)
        .collect()
}

/// 記事ファイルの状態（Done/None）とキューの状態を合わせる。保存済みなら常に Done。
fn resolve_summary_state(persisted: SummaryState, queued: SummaryState) -> SummaryState {
    if persisted == SummaryState::Done {
        SummaryState::Done
    } else {
        queued
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::article::ArticleReadState;
    use std::cell::RefCell;

    const SAVED: AutoSummaryOutcome = AutoSummaryOutcome::Saved;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn not_done(_: &str) -> bool {
        false
    }

    fn failure() -> Result<AutoSummaryOutcome, AppError> {
        Err(AppError::Validation("ai output rejected".to_string()))
    }

    /// 待機列が空になるまで処理し、summarize が呼ばれた順を返す。
    fn drain(
        core: &QueueCore,
        mut summarize: impl FnMut(&str) -> Result<AutoSummaryOutcome, AppError>,
    ) -> Vec<String> {
        let calls = RefCell::new(Vec::new());
        loop {
            let outcome = core.process_next(
                || true,
                not_done,
                |id| {
                    calls.borrow_mut().push(id.to_string());
                    summarize(id)
                },
            );
            if outcome == StepOutcome::Idle {
                return calls.into_inner();
            }
        }
    }

    #[test]
    fn processes_articles_in_the_given_recommendation_order() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["top", "second", "third"]));

        assert_eq!(
            drain(&core, |_| Ok(SAVED)),
            ids(&["top", "second", "third"])
        );
        assert_eq!(core.state_of("top"), SummaryState::None);
    }

    #[test]
    fn already_summarized_article_is_skipped_without_calling_ai() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["done-elsewhere", "next"]));

        // 手動要約・並べ直しの競合で要約済みになった記事は、AI を呼ばずに外す。
        let outcome = core.process_next(
            || true,
            |id| id == "done-elsewhere",
            |_| panic!("must not regenerate a summarized article"),
        );
        assert_eq!(outcome, StepOutcome::Skipped("done-elsewhere".to_string()));
        assert_eq!(core.state_of("done-elsewhere"), SummaryState::None);
        assert_eq!(drain(&core, |_| Ok(SAVED)), ids(&["next"]));
    }

    fn settings_with(enabled: bool, provider: &str, max: u32) -> PersistedSettings {
        let mut settings = PersistedSettings::default();
        settings.ai.auto_summary_enabled = enabled;
        settings.ai.provider = provider.to_string();
        settings.news.max_daily_recommendations = max;
        settings
    }

    fn with_key(_: AiProvider) -> bool {
        true
    }

    fn without_key(_: AiProvider) -> bool {
        false
    }

    #[test]
    fn auto_summary_runs_only_when_enabled_with_a_real_ai_provider() {
        for provider in ["gemini", "local"] {
            assert!(is_auto_summary_allowed(
                &settings_with(true, provider, 10),
                with_key
            ));
            assert!(!is_auto_summary_allowed(
                &settings_with(false, provider, 10),
                with_key
            ));
        }
        // Mock（未知の値も Mock 扱い）では有効でも動かない。
        assert!(!is_auto_summary_allowed(
            &settings_with(true, "mock", 10),
            with_key
        ));
        assert!(!is_auto_summary_allowed(
            &settings_with(true, "unknown", 10),
            with_key
        ));
        // 実AI呼び出しが未実装の openai も動かさない。
        assert!(!is_auto_summary_allowed(
            &settings_with(true, "openai", 10),
            with_key
        ));
    }

    #[test]
    fn auto_summary_does_not_run_when_the_real_ai_is_not_ready() {
        // 有効・Gemini でキーが未設定、有効・ローカルで同梱物が無いときは動かさない。
        for provider in ["gemini", "local"] {
            assert!(!is_auto_summary_allowed(
                &settings_with(true, provider, 10),
                without_key
            ));
        }
        // 確かめるのは設定中のプロバイダについてだけ。
        let checked = std::cell::Cell::new(None);
        is_auto_summary_allowed(&settings_with(true, "local", 10), |provider| {
            checked.set(Some(provider));
            true
        });
        assert_eq!(checked.get(), Some(AiProvider::Local));
        // 無効・他プロバイダのときは確かめない。
        for settings in [
            settings_with(false, "gemini", 10),
            settings_with(false, "local", 10),
            settings_with(true, "mock", 10),
        ] {
            assert!(!is_auto_summary_allowed(&settings, |_| panic!(
                "readiness check must not run when auto summary is off"
            )));
        }
    }

    #[test]
    fn local_uses_a_shorter_interval_than_gemini() {
        assert_eq!(
            item_interval_for(Some(AiProvider::Local)),
            LOCAL_ITEM_INTERVAL
        );
        assert_eq!(
            item_interval_for(Some(AiProvider::Gemini)),
            GEMINI_ITEM_INTERVAL
        );
        // 設定を読めないときは長い方（安全側）。
        assert_eq!(item_interval_for(None), GEMINI_ITEM_INTERVAL);
        assert!(LOCAL_ITEM_INTERVAL < GEMINI_ITEM_INTERVAL);
    }

    #[test]
    fn paused_queue_keeps_the_article_waiting_without_counting_an_attempt() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["a", "b"]));

        // 全画面・マイク使用中は取り出さない。記事は先頭に残り、失敗にも試行回数にも数えない。
        let outcome = core.process_next_unless_paused(
            || true,
            || true,
            not_done,
            |_| panic!("must not summarize while paused"),
        );
        assert_eq!(outcome, StepOutcome::Paused);
        assert_eq!(core.state_of("a"), SummaryState::Waiting);
        assert!(core.lock().failure_counts.is_empty());
        assert!(core.failed_ids().is_empty());

        // 解除されたら、止めた記事から順に処理する。
        let outcome = core.process_next_unless_paused(|| true, || false, not_done, |_| Ok(SAVED));
        assert_eq!(outcome, StepOutcome::Succeeded("a".to_string()));
        assert_eq!(drain(&core, |_| Ok(SAVED)), ids(&["b"]));
    }

    #[test]
    fn pause_check_is_not_queried_when_nothing_is_waiting() {
        let core = QueueCore::default();
        let outcome = core.process_next_unless_paused(
            || true,
            || panic!("must not query the os when the queue is empty"),
            not_done,
            |_| panic!("nothing to summarize"),
        );
        assert_eq!(outcome, StepOutcome::Idle);
    }

    /// 一時ディレクトリに記事（シード）と設定を置いた実キュー。終了時に片付ける。
    struct QueueContext {
        queue: AutoSummaryQueue,
        root: std::path::PathBuf,
    }

    impl Drop for QueueContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn queue_context(name: &str, key_available: bool) -> QueueContext {
        queue_context_with(name, "gemini", key_available)
    }

    fn queue_context_with(name: &str, provider: &str, ai_ready: bool) -> QueueContext {
        use crate::paths::AppPaths;
        use crate::repositories::article_repository::ArticleRepository;
        use crate::services::ai_provider_service::AiProviderService;

        let root = std::env::temp_dir().join(format!(
            "auto-summary-queue-tests-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths::new(root.clone());
        paths.ensure_storage_dirs().expect("create storage dirs");
        let article_repository = ArticleRepository::new(&paths);
        article_repository.seed_sample_articles_for_tests();
        // 取得直後と同じ、未要約の記事を1件置く。
        let fetched_at = chrono::Utc::now().to_rfc3339();
        article_repository
            .save_fetched_articles(vec![crate::domain::article::FetchedArticle {
                article_id: "unsummarized".to_string(),
                title: "テスト記事".to_string(),
                source_name: "テストソース".to_string(),
                original_url: "https://example.com/unsummarized".to_string(),
                fetched_at: fetched_at.clone(),
                published_at_text: fetched_at,
                genre: "テクノロジー".to_string(),
                tags: Vec::new(),
                excerpt: Some("抜粋".to_string()),
                recommendation_score: 0.5,
                read_state: ArticleReadState::Unread,
            }])
            .expect("save fetched article");
        let settings_repository = SettingsRepository::new(&paths);
        settings_repository
            .save(&settings_with(true, provider, 10))
            .expect("save settings");
        let queue = AutoSummaryQueue::new(
            ArticleService::new(ArticleRepository::new(&paths)),
            SummaryService::new(
                AiProviderService::new(&paths),
                article_repository,
                SettingsRepository::new(&paths),
            ),
            settings_repository,
        )
        .with_ai_check(move |_| ai_ready)
        .with_busy_check(|| false);
        QueueContext { queue, root }
    }

    #[test]
    fn local_queue_enqueues_only_when_the_bundle_is_present() {
        // 同梱物がそろっていれば並べて処理する。
        let ctx = queue_context_with("local-present", "local", true);
        let queue = &ctx.queue;
        queue.enqueue_unsummarized();
        assert_eq!(queue.core.state_of("unsummarized"), SummaryState::Waiting);
        let first = queue.core.lock().pending.front().cloned().unwrap();
        let outcome = queue.core.process_next_unless_paused(
            || queue.is_enabled(),
            || queue.is_paused(),
            not_done,
            |_| Ok(SAVED),
        );
        assert_eq!(outcome, StepOutcome::Succeeded(first));
        assert_eq!(queue.item_interval(), LOCAL_ITEM_INTERVAL);

        // 同梱物が無ければ並べない（LLM を起動しようとして失敗を繰り返さない）。
        let ctx = queue_context_with("local-missing", "local", false);
        ctx.queue.enqueue_unsummarized();
        assert!(ctx.queue.core.lock().pending.is_empty());
    }

    #[test]
    fn local_queue_pauses_while_busy_but_gemini_does_not() {
        let ctx = queue_context_with("local-busy", "local", true);
        let queue = ctx.queue.clone().with_busy_check(|| true);
        queue.enqueue_unsummarized();
        let outcome = queue.core.process_next_unless_paused(
            || queue.is_enabled(),
            || queue.is_paused(),
            not_done,
            |_| panic!("must not run the local llm while the pc is busy"),
        );
        assert_eq!(outcome, StepOutcome::Paused);
        assert_eq!(queue.core.state_of("unsummarized"), SummaryState::Waiting);

        // Gemini は CPU を使わないため、全画面・マイク使用中でも止めない。
        let ctx = queue_context_with("gemini-busy", "gemini", true);
        let queue = ctx.queue.clone().with_busy_check(|| true);
        assert!(!queue.is_paused());
        assert_eq!(queue.item_interval(), GEMINI_ITEM_INTERVAL);
    }

    #[test]
    fn queue_enqueues_and_processes_when_gemini_key_is_configured() {
        let ctx = queue_context("with-key", true);
        let queue = &ctx.queue;
        queue.enqueue_unsummarized();
        assert_eq!(
            queue.core.state_of("unsummarized"),
            SummaryState::Waiting,
            "unsummarized article is enqueued"
        );
        let first = queue.core.lock().pending.front().cloned().unwrap();

        // 実AIは呼ばず、要約処理を差し替えて取り出しまでを確かめる。
        let outcome = queue
            .core
            .process_next(|| queue.is_enabled(), not_done, |_| Ok(SAVED));
        assert_eq!(outcome, StepOutcome::Succeeded(first));
    }

    #[test]
    fn queue_neither_enqueues_nor_processes_without_gemini_key() {
        let ctx = queue_context("without-key", false);
        let queue = &ctx.queue;
        queue.enqueue_unsummarized();
        assert!(queue.core.lock().pending.is_empty());

        // キーが外れる前に並んでいた記事も、取り出し時の確認で捨てて処理しない。
        queue.core.replace_pending(ids(&["queued-before"]));
        let outcome = queue.core.process_next(
            || queue.is_enabled(),
            not_done,
            |_| panic!("must not summarize without a gemini key"),
        );
        assert_eq!(outcome, StepOutcome::Disabled);
        assert!(queue.core.lock().pending.is_empty());
        assert_eq!(queue.core.state_of("queued-before"), SummaryState::None);
    }

    #[test]
    fn enqueue_is_capped_to_max_daily_recommendations_in_order() {
        let ordered = ids(&["a", "b", "c", "d"]);
        assert_eq!(
            cap_for_enqueue(
                ordered.clone(),
                &settings_with(true, "gemini", 2),
                &HashSet::new()
            ),
            ids(&["a", "b"])
        );
        assert_eq!(
            cap_for_enqueue(
                ordered.clone(),
                &settings_with(true, "gemini", 10),
                &HashSet::new()
            ),
            ordered
        );
    }

    #[test]
    fn enqueue_cap_does_not_count_failed_articles() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["bad", "a"]));
        drain(&core, |id| if id == "bad" { failure() } else { Ok(SAVED) });
        assert_eq!(core.state_of("bad"), SummaryState::Failed);

        // 上限2件でも、上限まで失敗した記事は枠に数えず、その次の記事までを並べる。
        let failed = core.failed_ids();
        let capped = cap_for_enqueue(
            ids(&["bad", "b", "c", "d"]),
            &settings_with(true, "gemini", 2),
            &failed,
        );
        assert_eq!(capped, ids(&["b", "c"]));
        core.replace_pending(capped);
        assert_eq!(drain(&core, |_| Ok(SAVED)), ids(&["b", "c"]));
    }

    #[test]
    fn article_summarized_manually_during_the_ai_call_is_done_not_failed() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["raced", "next"]));

        // 保存直前の確認で要約済みと分かった（上書きしなかった）記事は、失敗に数えず完了として外す。
        let outcome = core.process_next(
            || true,
            not_done,
            |_| Ok(AutoSummaryOutcome::AlreadySummarized),
        );
        assert_eq!(outcome, StepOutcome::AlreadySummarized("raced".to_string()));
        assert_eq!(core.state_of("raced"), SummaryState::None);
        assert!(core.lock().failure_counts.is_empty());
        assert!(core.failed_ids().is_empty());
        assert_eq!(drain(&core, |_| Ok(SAVED)), ids(&["next"]));
    }

    #[test]
    fn requeue_after_refresh_follows_the_latest_order_and_skips_processing() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["a", "b"]));
        core.process_next(
            || true,
            not_done,
            |id| {
                // 処理中に取得が終わり並べ直されても、処理中の記事は二重に並ばない。
                core.replace_pending(ids(&["c", id, "b"]));
                Ok(SAVED)
            },
        );
        assert_eq!(drain(&core, |_| Ok(SAVED)), ids(&["c", "b"]));
    }

    #[test]
    fn only_one_article_is_processed_at_a_time() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["a", "b", "c"]));

        let outcome = core.process_next(
            || true,
            not_done,
            |id| {
                assert_eq!(core.state_of(id), SummaryState::Processing);
                assert_eq!(core.state_of("b"), SummaryState::Waiting);
                assert_eq!(core.state_of("c"), SummaryState::Waiting);
                // 処理中にもう1件取り出そうとしても取り出さない。
                let nested =
                    core.process_next(|| true, not_done, |_| panic!("must not run concurrently"));
                assert_eq!(nested, StepOutcome::Idle);
                assert!(!core.lock().pending.is_empty());
                Ok(SAVED)
            },
        );
        assert_eq!(outcome, StepOutcome::Succeeded("a".to_string()));
        assert_eq!(core.state_of("b"), SummaryState::Waiting);
    }

    #[test]
    fn failed_article_is_retried_up_to_three_times_without_blocking_others() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["bad", "good1", "good2"]));

        let calls = drain(&core, |id| if id == "bad" { failure() } else { Ok(SAVED) });

        // 失敗した記事は後ろへ回り、他の記事が先に処理される。合計3回で打ち切る。
        assert_eq!(calls, ids(&["bad", "good1", "good2", "bad", "bad"]));
        assert_eq!(core.state_of("bad"), SummaryState::Failed);
        assert_eq!(core.state_of("good1"), SummaryState::None);

        // 上限到達後は再投入しても並ばない（再起動まで Failed）。
        core.replace_pending(ids(&["bad", "next"]));
        assert_eq!(drain(&core, |_| Ok(SAVED)), ids(&["next"]));
        assert_eq!(core.state_of("bad"), SummaryState::Failed);
    }

    #[test]
    fn article_waiting_for_retry_is_shown_as_waiting() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["flaky"]));
        let outcome = core.process_next(|| true, not_done, |_| failure());
        assert_eq!(outcome, StepOutcome::Retrying("flaky".to_string()));
        assert_eq!(core.state_of("flaky"), SummaryState::Waiting);

        // 2回目で成功すれば失敗回数は消える。
        assert_eq!(
            core.process_next(|| true, not_done, |_| Ok(SAVED)),
            StepOutcome::Succeeded("flaky".to_string())
        );
        assert!(core.lock().failure_counts.is_empty());
    }

    #[test]
    fn nothing_is_processed_when_auto_summary_is_disabled() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["a", "b"]));

        let outcome = core.process_next(
            || false,
            not_done,
            |_| panic!("must not summarize when disabled"),
        );

        assert_eq!(outcome, StepOutcome::Disabled);
        assert_eq!(core.state_of("a"), SummaryState::None);
        assert!(!core.lock().pending.iter().any(|_| true));
    }

    #[test]
    fn stop_request_prevents_taking_new_articles() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["a", "b"]));
        core.request_stop();

        let outcome = core.process_next(|| true, not_done, |_| panic!("must not start after stop"));
        assert_eq!(outcome, StepOutcome::Stopped);
        assert!(!core.wait_for_work());
    }

    #[test]
    fn stop_during_processing_finishes_the_current_article_and_takes_no_more() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["a", "b"]));

        let outcome = core.process_next(
            || true,
            not_done,
            |_| {
                core.request_stop();
                Ok(SAVED)
            },
        );

        assert_eq!(outcome, StepOutcome::Succeeded("a".to_string()));
        assert_eq!(core.state_of("a"), SummaryState::None);
        assert_eq!(
            core.process_next(
                || true,
                not_done,
                |_| panic!("must not continue after stop")
            ),
            StepOutcome::Stopped
        );
    }

    #[test]
    fn stop_wakes_up_a_waiting_worker_promptly() {
        let core = Arc::new(QueueCore::default());
        let waiter = {
            let core = Arc::clone(&core);
            std::thread::spawn(move || {
                core.wait_interval(Duration::from_secs(60));
                core.wait_for_work()
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        core.request_stop();
        assert!(!waiter.join().unwrap());
    }

    fn summary_dto(id: &str, state: SummaryState) -> ArticleSummaryDto {
        ArticleSummaryDto {
            article_id: id.to_string(),
            title: "title".to_string(),
            source_name: "source".to_string(),
            published_at_text: "today".to_string(),
            genre: "AI".to_string(),
            summary: None,
            is_favorite: false,
            read_state: ArticleReadState::Unread,
            recommendation_score: 0.5,
            summary_state: state,
        }
    }

    #[test]
    fn persisted_done_always_wins_over_queue_state() {
        assert_eq!(
            resolve_summary_state(SummaryState::Done, SummaryState::Failed),
            SummaryState::Done
        );
        assert_eq!(
            resolve_summary_state(SummaryState::None, SummaryState::Waiting),
            SummaryState::Waiting
        );
        assert_eq!(
            resolve_summary_state(SummaryState::None, SummaryState::None),
            SummaryState::None
        );
    }

    #[test]
    fn summary_state_is_exposed_as_camel_case_field_with_lowercase_values() {
        let value = serde_json::to_value(summary_dto("a", SummaryState::Processing)).unwrap();
        assert_eq!(value["summaryState"], "processing");
        for (state, expected) in [
            (SummaryState::None, "none"),
            (SummaryState::Waiting, "waiting"),
            (SummaryState::Done, "done"),
            (SummaryState::Failed, "failed"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), expected);
        }

        // 旧データ（ゆうこ状態ファイル内の記事）に欄が無くても読み込め、None になる。
        let mut legacy = value;
        legacy.as_object_mut().unwrap().remove("summaryState");
        let parsed: ArticleSummaryDto = serde_json::from_value(legacy).unwrap();
        assert_eq!(parsed.summary_state, SummaryState::None);
    }

    #[test]
    fn history_item_exposes_summary_state_for_list_tags() {
        let item = ArticleHistoryItemDto {
            article_id: "a".to_string(),
            title: "title".to_string(),
            source_name: "source".to_string(),
            published_at_text: "today".to_string(),
            fetched_at: "2026-10-09T00:00:00+09:00".to_string(),
            genre: "AI".to_string(),
            summary: None,
            is_favorite: false,
            read_state: ArticleReadState::Unread,
            is_archived: false,
            recommendation_score: 0.5,
            summary_state: SummaryState::Waiting,
            tags: Vec::new(),
        };
        let value = serde_json::to_value(&item).unwrap();
        assert_eq!(value["summaryState"], "waiting");
    }

    // --- 待機列が空になったら、ローカルLLMへ早めの停止を頼む ---

    fn counting_queue(name: &str) -> (QueueContext, Arc<std::sync::atomic::AtomicUsize>) {
        let mut ctx = queue_context_with(name, "local", true);
        let drained = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = drained.clone();
        ctx.queue = ctx.queue.clone().with_drained_hook(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        (ctx, drained)
    }

    fn step_and_notify(
        queue: &AutoSummaryQueue,
        summarize: impl FnOnce(&str) -> Result<AutoSummaryOutcome, AppError>,
    ) -> StepOutcome {
        let outcome = queue
            .core
            .process_next_unless_paused(|| true, || false, not_done, summarize);
        queue.notify_if_drained(&outcome);
        outcome
    }

    #[test]
    fn drained_hook_fires_once_when_the_last_item_is_done() {
        let (ctx, drained) = counting_queue("drained-once");
        let queue = &ctx.queue;
        queue.core.replace_pending(ids(&["a", "b"]));

        assert_eq!(
            step_and_notify(queue, |_| Ok(SAVED)),
            StepOutcome::Succeeded("a".to_string())
        );
        // まだ b が待っている。
        assert_eq!(drained.load(Ordering::SeqCst), 0);
        assert_eq!(
            step_and_notify(queue, |_| Ok(SAVED)),
            StepOutcome::Succeeded("b".to_string())
        );
        assert_eq!(drained.load(Ordering::SeqCst), 1);
        // 取り出すものが無い回（Idle）では繰り返し知らせない。
        assert_eq!(step_and_notify(queue, |_| Ok(SAVED)), StepOutcome::Idle);
        assert_eq!(drained.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn drained_hook_waits_for_retries_and_fires_after_giving_up() {
        let (ctx, drained) = counting_queue("drained-retry");
        let queue = &ctx.queue;
        queue.core.replace_pending(ids(&["a"]));
        let fail = |_: &str| Err(AppError::Validation("x".to_string()));
        for _ in 1..MAX_ATTEMPTS {
            assert_eq!(
                step_and_notify(queue, fail),
                StepOutcome::Retrying("a".to_string())
            );
            // 再試行で待機列へ戻ったので、まだ空ではない。
            assert_eq!(drained.load(Ordering::SeqCst), 0);
        }
        assert_eq!(
            step_and_notify(queue, fail),
            StepOutcome::Failed("a".to_string())
        );
        assert_eq!(drained.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn drained_hook_is_not_called_while_paused() {
        let (ctx, drained) = counting_queue("drained-paused");
        let queue = &ctx.queue;
        queue.core.replace_pending(ids(&["a"]));
        let outcome =
            queue
                .core
                .process_next_unless_paused(|| true, || true, not_done, |_| panic!("paused"));
        queue.notify_if_drained(&outcome);
        assert_eq!(outcome, StepOutcome::Paused);
        assert_eq!(drained.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn only_steps_that_took_an_item_can_report_a_drained_queue() {
        for outcome in [
            StepOutcome::Succeeded("a".to_string()),
            StepOutcome::Skipped("a".to_string()),
            StepOutcome::AlreadySummarized("a".to_string()),
            StepOutcome::YieldedToManual("a".to_string()),
            StepOutcome::Retrying("a".to_string()),
            StepOutcome::Failed("a".to_string()),
            StepOutcome::Disabled,
        ] {
            assert!(took_an_item(&outcome), "{outcome:?}");
        }
        for outcome in [
            StepOutcome::Idle,
            StepOutcome::Stopped,
            StepOutcome::Paused,
            StepOutcome::SettingsUnavailable,
        ] {
            assert!(!took_an_item(&outcome), "{outcome:?}");
        }
    }

    #[test]
    fn local_llm_release_hook_is_harmless_when_the_server_is_not_running() {
        let ctx = queue_context_with("drained-local", "local", true);
        let queue = ctx
            .queue
            .clone()
            .with_local_llm(LocalLlmService::unavailable());
        queue.core.replace_pending(ids(&["a"]));
        assert_eq!(
            step_and_notify(&queue, |_| Ok(SAVED)),
            StepOutcome::Succeeded("a".to_string())
        );
    }

    // --- 手動の要約・手動の要求との協調 ---

    #[test]
    fn article_being_summarized_manually_is_skipped_without_calling_ai() {
        let ctx = queue_context_with("manual-in-flight", "local", true);
        let queue = &ctx.queue;
        queue.enqueue_unsummarized();
        assert_eq!(queue.core.state_of("unsummarized"), SummaryState::Waiting);

        // 「今すぐ要約」で同じ記事を生成中（まだ保存前）。キューは AI を呼ばずに外す。
        let manual = queue
            .summary_service
            .mark_manual_summary_in_progress("unsummarized");
        let outcome = queue.core.process_next_unless_paused(
            || queue.is_enabled(),
            || false,
            |id| queue.is_done_or_taken(id),
            |_| panic!("must not generate an article that is being summarized manually"),
        );
        assert_eq!(outcome, StepOutcome::Skipped("unsummarized".to_string()));
        assert_eq!(queue.core.state_of("unsummarized"), SummaryState::None);
        assert!(queue.core.lock().failure_counts.is_empty());

        // 手動の要約が終われば（失敗しても）、次の投入でまた並ぶ。
        drop(manual);
        assert!(!queue
            .summary_service
            .is_manual_summary_in_progress("unsummarized"));
        queue.enqueue_unsummarized();
        assert_eq!(queue.core.state_of("unsummarized"), SummaryState::Waiting);
    }

    #[test]
    fn yielding_to_a_manual_summary_is_not_a_failure() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["taken", "next"]));

        let outcome = core.process_next(
            || true,
            not_done,
            |_| Ok(AutoSummaryOutcome::YieldedToManual),
        );
        assert_eq!(outcome, StepOutcome::YieldedToManual("taken".to_string()));
        assert_eq!(core.state_of("taken"), SummaryState::None);
        assert!(core.lock().failure_counts.is_empty());
        assert!(core.failed_ids().is_empty());
        assert_eq!(drain(&core, |_| Ok(SAVED)), ids(&["next"]));
    }

    #[test]
    fn local_llm_wait_is_asked_only_for_the_local_provider_and_clamped() {
        // ローカル以外（Gemini・設定を読めない）では尋ねない。
        for provider in [None, Some(AiProvider::Gemini), Some(AiProvider::Mock)] {
            assert_eq!(
                local_llm_wait_for(provider, || panic!("must not ask the local llm")),
                None
            );
        }
        let local = Some(AiProvider::Local);
        assert_eq!(local_llm_wait_for(local, || None), None);
        assert_eq!(
            local_llm_wait_for(local, || Some(Duration::from_secs(30))),
            Some(Duration::from_secs(30))
        );
        // 起動を控える 10 分は 60 秒ずつに区切り、ごく短い残りは 1 秒にまとめる。
        assert_eq!(
            local_llm_wait_for(local, || Some(Duration::from_secs(600))),
            Some(LOCAL_LLM_RECHECK_MAX)
        );
        assert_eq!(
            local_llm_wait_for(local, || Some(Duration::from_millis(5))),
            Some(LOCAL_LLM_RECHECK_MIN)
        );
    }

    #[test]
    fn queue_asks_the_local_llm_before_taking_the_next_article() {
        let ctx = queue_context_with("llm-wait-local", "local", true);
        let queue = ctx
            .queue
            .clone()
            .with_local_llm_wait(|| Some(Duration::from_secs(20)));
        assert_eq!(queue.local_llm_wait(), Some(Duration::from_secs(20)));

        // Gemini の設定では、ローカルLLMの都合で待たない。
        let ctx = queue_context_with("llm-wait-gemini", "gemini", true);
        let queue = ctx
            .queue
            .clone()
            .with_local_llm_wait(|| panic!("must not ask the local llm for gemini"));
        assert_eq!(queue.local_llm_wait(), None);
    }

    #[test]
    fn real_local_llm_wait_is_none_when_nothing_happened() {
        // 手動の要求も起動失敗も無ければ待たない（本番の窓口をそのままつなぐ）。
        let ctx = queue_context_with("llm-wait-real", "local", true);
        let queue = ctx
            .queue
            .clone()
            .with_local_llm(LocalLlmService::unavailable());
        assert_eq!(queue.local_llm_wait(), None);
    }

    #[test]
    fn settings_read_error_keeps_the_queue_and_counts_nothing() {
        let core = QueueCore::default();
        core.replace_pending(ids(&["a", "b"]));

        // 設定が読めない間は取り出さず、待機列も捨てない（無効とは扱わない）。
        let outcome = core.process_next(
            || Enablement::Unknown,
            not_done,
            |_| panic!("must not summarize while settings are unreadable"),
        );
        assert_eq!(outcome, StepOutcome::SettingsUnavailable);
        assert!(!took_an_item(&outcome));
        assert_eq!(core.state_of("a"), SummaryState::Waiting);
        assert_eq!(core.state_of("b"), SummaryState::Waiting);
        assert!(core.lock().failure_counts.is_empty());

        // 読めるようになれば、残っていた順に処理する。
        assert_eq!(drain(&core, |_| Ok(SAVED)), ids(&["a", "b"]));

        // 本当に無効なら、従来どおり待機列を捨てる。
        core.replace_pending(ids(&["c"]));
        assert_eq!(
            core.process_next(|| Enablement::Disabled, not_done, |_| Ok(SAVED)),
            StepOutcome::Disabled
        );
        assert!(core.lock().pending.is_empty());
    }

    /// 初回起動と同じ順序・同じ配線（lib.rs）で作った実キュー。同梱物は小さな偽物を一時フォルダへ置く。
    struct FirstRunContext {
        queue: AutoSummaryQueue,
        root: std::path::PathBuf,
    }

    impl Drop for FirstRunContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn first_run_context(name: &str, fetched: usize) -> FirstRunContext {
        use crate::infra::local_llm_runtime::{ChildProcessLauncher, LocalLlmBundle};
        use crate::paths::AppPaths;
        use crate::repositories::article_repository::ArticleRepository;
        use crate::services::ai_provider_service::AiProviderService;
        use crate::services::local_llm_service::LocalLlmConfig;
        use crate::services::settings_service::SettingsService;

        let root = std::env::temp_dir().join(format!(
            "auto-summary-first-run-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths::new(root.join("app-data"));
        paths.ensure_storage_dirs().expect("create storage dirs");
        // 起動時と同じく、設定ファイルが無ければ初期値（ローカル・自動要約オン）で作る。
        SettingsService::new(SettingsRepository::new(&paths))
            .initialize_default_if_missing()
            .expect("initialize settings");
        let article_repository = ArticleRepository::new(&paths);
        article_repository
            .initialize_default_if_missing()
            .expect("initialize articles");
        let fetched_at = chrono::Utc::now().to_rfc3339();
        let articles = (0..fetched)
            .map(|index| crate::domain::article::FetchedArticle {
                article_id: format!("fetched-{index:03}"),
                title: format!("取得した記事 {index}"),
                source_name: "テストソース".to_string(),
                original_url: format!("https://example.com/fetched/{index}"),
                fetched_at: fetched_at.clone(),
                published_at_text: fetched_at.clone(),
                genre: "テクノロジー".to_string(),
                tags: Vec::new(),
                excerpt: Some("抜粋".to_string()),
                recommendation_score: 0.5,
                read_state: ArticleReadState::Unread,
            })
            .collect();
        article_repository
            .save_fetched_articles(articles)
            .expect("save fetched articles");

        // 同梱物の形だけを置く（起動はしない。大きさの確認だけが通るようにする）。
        let bundle_dir = root.join("local_llm");
        let bundle = LocalLlmBundle::in_dir(&bundle_dir);
        std::fs::create_dir_all(bundle.exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(bundle.model.parent().unwrap()).unwrap();
        std::fs::write(&bundle.exe, b"exe").unwrap();
        std::fs::write(&bundle.model, b"model").unwrap();
        let local_llm = LocalLlmService::with_parts(
            Some(bundle_dir),
            Arc::new(ChildProcessLauncher),
            LocalLlmConfig {
                model_size: 5,
                ..LocalLlmConfig::default()
            },
        );
        let ai_provider_service = AiProviderService::new(&paths).with_local_llm(local_llm.clone());
        let queue = AutoSummaryQueue::new(
            ArticleService::new(ArticleRepository::new(&paths)),
            SummaryService::new(
                ai_provider_service,
                article_repository,
                SettingsRepository::new(&paths),
            ),
            SettingsRepository::new(&paths),
        )
        .with_local_llm(local_llm)
        .with_busy_check(|| false);
        FirstRunContext { queue, root }
    }

    #[test]
    fn first_run_enqueues_fetched_articles_with_the_initial_local_settings() {
        let ctx = first_run_context("enqueue", 60);
        let queue = &ctx.queue;
        queue.enqueue_unsummarized();
        assert_eq!(queue.core.lock().pending.len(), 10);
        assert_eq!(*queue.last_notice.lock().unwrap(), None);
        let first = queue.core.lock().pending.front().cloned().unwrap();
        assert_eq!(queue.local_llm_wait(), None);
        let outcome = queue.core.process_next_unless_paused(
            || queue.is_enabled(),
            || queue.is_paused(),
            |article_id| queue.is_done_or_taken(article_id),
            |_| Ok(SAVED),
        );
        assert_eq!(outcome, StepOutcome::Succeeded(first));
    }

    #[test]
    fn off_reason_names_why_auto_summary_does_not_run() {
        assert_eq!(
            auto_summary_off_reason(&settings_with(false, "local", 10), with_key),
            Some(OffReason::DisabledInSettings)
        );
        assert_eq!(
            auto_summary_off_reason(&settings_with(true, "mock", 10), with_key),
            Some(OffReason::NotRealAi(AiProvider::Mock))
        );
        assert_eq!(
            auto_summary_off_reason(&settings_with(true, "local", 10), without_key),
            Some(OffReason::AiNotReady(AiProvider::Local))
        );
        assert_eq!(
            auto_summary_off_reason(&settings_with(true, "local", 10), with_key),
            None
        );
        // ログの理由はプロバイダ名と固定の分類だけ（キー・場所は含まない）。
        assert_eq!(
            OffReason::AiNotReady(AiProvider::Gemini).describe(),
            "reason=ai_not_ready provider=Gemini"
        );
    }

    #[test]
    fn the_same_notice_is_logged_only_once_in_a_row() {
        let ctx = queue_context_with("notice-repeat", "local", true);
        let queue = &ctx.queue;
        assert!(queue.note(QueueNotice::Running));
        assert!(!queue.note(QueueNotice::Running));
        assert!(queue.note(QueueNotice::Paused(BusyCause::Fullscreen)));
        assert!(!queue.note(QueueNotice::Paused(BusyCause::Fullscreen)));
        assert!(queue.note(QueueNotice::Paused(BusyCause::MicInUse)));
        // 止まっていた後に動き出したら、もう一度残す。
        assert!(queue.note(QueueNotice::Running));
        assert!(queue.note(QueueNotice::Drained));
        assert!(!queue.note(QueueNotice::Drained));
    }

    #[test]
    fn off_decision_is_noted_once_and_forgotten_when_allowed_again() {
        let ctx = queue_context_with("notice-off", "local", false);
        let off = QueueNotice::Off(OffReason::AiNotReady(AiProvider::Local));
        ctx.queue.enqueue_unsummarized();
        assert_eq!(*ctx.queue.last_notice.lock().unwrap(), Some(off));
        // 取り出し側の確認（is_enabled）でも同じ理由なら繰り返さない。
        assert_eq!(ctx.queue.is_enabled(), Enablement::Disabled);
        assert!(!ctx.queue.note(off));

        // 呼べるようになれば並べ、「動かさない」の記録を外す（また止まったら残せるように）。
        let ready = ctx.queue.clone().with_ai_check(|_| true);
        ready.enqueue_unsummarized();
        assert_eq!(*ready.last_notice.lock().unwrap(), None);
        assert!(!ready.core.lock().pending.is_empty());
    }

    #[test]
    fn paused_queue_notes_the_busy_cause() {
        let ctx = queue_context_with("notice-paused", "local", true);
        let queue = ctx.queue.clone().with_busy_check(|| true);
        assert!(queue.is_paused());
        assert_eq!(
            *queue.last_notice.lock().unwrap(),
            Some(QueueNotice::Paused(BusyCause::Test))
        );
    }

    #[test]
    fn settings_retry_interval_backs_off_to_ten_minutes() {
        let secs = |streak| settings_retry_interval(streak).as_secs();
        assert_eq!(secs(0), 60);
        assert_eq!(secs(1), 60);
        assert_eq!(secs(2), 120);
        assert_eq!(secs(3), 240);
        assert_eq!(secs(4), 480);
        assert_eq!(secs(5), 600);
        assert_eq!(secs(100), 600);
        assert_eq!(secs(u32::MAX), 600);
    }
}
