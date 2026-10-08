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
//! - 外部AIの利用枠を使い切らないよう、設定 `ai.autoSummaryEnabled` が有効なときだけ動く（既定は無効）。
//! - 設定の AI プロバイダが実装済みの実AI（現在は Gemini）以外のときは動かない（判断台帳 D56）。固定応答で記事を「要約済み」に
//!   してしまうと、後で実AIを使えるようになっても自動では作り直されないため。
//! - 実AIの失敗・利用枠超過・検証落ちで Mock の代替出力になった場合は保存せず失敗として扱い、
//!   再試行→失敗の流れに乗せる（`generate_article_summary_without_fallback`、D56）。
//! - 1回の投入はおすすめ順の上位 `maxDailyRecommendations` 件まで（常駐負荷を抑えるため、D56）。
//! - 終了時は新しい記事を取り出さない。処理中の1件は待たずに終了してよい。記事の保存は
//!   一時ファイルへ書き切ってから tmp→bak→本体 の順に差し替える方式（article_repository の
//!   atomic_write）のため、書きかけの .md は残らない。ただし2回の rename の間でプロセスが
//!   強制終了されると、.bak だけが残る短い隙間がある（既存の全保存処理に共通の制約）。
//! - 取り出した記事は AI を呼ぶ前に記事ファイルで要約済みか確かめ直し、手動要約や並べ直しとの
//!   競合で要約済みになった記事を上書きしない。AI 呼び出し中に手動要約された場合も、保存の直前に
//!   同じ書き込みロック内で確かめ直して上書きせず、失敗ではなく完了として外す。
//! - 実AIの出力が1つでも Mock に切り替わったら、その記事の残りの AI 呼び出しはしない
//!   （キー未設定・利用枠超過で、保存しない出力のために外部AIを呼び続けないため）。
//! - 上限まで失敗した記事は投入件数の上限に数えない（失敗した記事で枠が埋まり、他の記事が
//!   自動要約されなくなるのを防ぐため）。
//! - AI 呼び出し中はキューのロックを持たない（画面からの状態取得を待たせない）。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use crate::domain::article::{ArticleDetailDto, ArticleSummaryDto, SummaryState};
use crate::domain::settings::{AiProvider, PersistedSettings};
use crate::domain::summary::GenerateArticleSummaryParams;
use crate::error::AppError;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::article_service::ArticleService;
use crate::services::summary_service::{AutoSummaryOutcome, SummaryService};

/// 1記事あたりの試行回数の上限（初回を含めた合計）。
const MAX_ATTEMPTS: u32 = 3;

/// 起動後、最初の1件を処理するまでの待ち時間。起動時のニュース取得・画面表示と競合させず、
/// OS 自動起動の直後でネットワークがまだ使えない時間帯に AI を呼ばないため 60 秒待つ。
const STARTUP_DELAY: Duration = Duration::from_secs(60);

/// 1件処理するごとに空ける間隔。1件で AI を3回呼ぶため、Gemini の無料枠の毎分上限に
/// 近づかないよう、30秒空けて「1分あたり最大2件（AI呼び出し6回）」程度に抑える。
const ITEM_INTERVAL: Duration = Duration::from_secs(30);

/// 1回の処理ステップの結果。ログとテストで使う。
#[derive(Debug, Clone, PartialEq, Eq)]
enum StepOutcome {
    /// 取り出す記事が無い、または処理中の記事がある（同時に2件は処理しない）。
    Idle,
    /// 終了要求済みのため取り出さなかった。
    Stopped,
    /// 自動要約が無効のため、待機中の記事を捨てて何もしなかった。
    Disabled,
    Succeeded(String),
    /// 取り出した時点で既に要約済みだったため、AI を呼ばずに外した。
    Skipped(String),
    /// AI 呼び出し中に手動要約などで要約済みになったため、保存せずに外した（失敗に数えない）。
    AlreadySummarized(String),
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

    /// 次の1件を処理する。`summarize` の実行中はロックを持たない。
    /// `is_done` は記事ファイルで要約済みかを副作用なしに確かめる（既読状態は進めない）。
    fn process_next<E, D, S>(&self, is_enabled: E, is_done: D, summarize: S) -> StepOutcome
    where
        E: FnOnce() -> bool,
        D: FnOnce(&str) -> bool,
        S: FnOnce(&str) -> Result<AutoSummaryOutcome, AppError>,
    {
        if self.stopped.load(Ordering::SeqCst) {
            return StepOutcome::Stopped;
        }
        if !is_enabled() {
            self.clear_pending();
            return StepOutcome::Disabled;
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

/// 自動要約キュー。AppState と NewsScheduler で共有する（clone しても同じキューを指す）。
#[derive(Clone)]
pub struct AutoSummaryQueue {
    core: Arc<QueueCore>,
    article_service: ArticleService,
    summary_service: SummaryService,
    settings_repository: SettingsRepository,
}

impl AutoSummaryQueue {
    pub fn new(
        article_service: ArticleService,
        summary_service: SummaryService,
        settings_repository: SettingsRepository,
    ) -> Self {
        Self {
            core: Arc::new(QueueCore::default()),
            article_service,
            summary_service,
            settings_repository,
        }
    }

    /// 未要約の記事をおすすめ順で待機列へ並べ直す。ニュース取得の完了後・起動時・設定保存後に呼ぶ。
    /// 無効時は待機列を空にする（処理中の1件はそのまま終わらせる）。
    pub fn enqueue_unsummarized(&self) {
        let Some(settings) = self.load_settings() else {
            self.core.clear_pending();
            return;
        };
        if !is_auto_summary_allowed(&settings) {
            self.core.clear_pending();
            return;
        }
        match self.article_service.list_unsummarized_article_ids() {
            Ok(ids) => {
                let failed = self.core.failed_ids();
                self.core
                    .replace_pending(cap_for_enqueue(ids, &settings, &failed));
            }
            Err(error) => log::warn!("failed to list unsummarized articles: {error}"),
        }
    }

    /// 専用スレッドでワーカーを開始する。起動時に残っている未要約記事も対象にする。
    pub fn start(&self) {
        let queue = self.clone();
        std::thread::spawn(move || {
            // 終了要求があれば待ちを打ち切る（その後の wait_for_work が false を返して終わる）。
            queue.core.wait_interval(STARTUP_DELAY);
            queue.enqueue_unsummarized();
            queue.run_worker();
        });
    }

    fn run_worker(&self) {
        while self.core.wait_for_work() {
            let outcome = self.core.process_next(
                || self.is_enabled(),
                |article_id| self.is_summarized(article_id),
                |article_id| {
                    self.summary_service
                        .generate_article_summary_without_fallback(GenerateArticleSummaryParams {
                            article_id: article_id.to_string(),
                        })
                },
            );
            match outcome {
                // AI を呼んだ後は、保存しなかった場合も含めて間隔を空ける（外部AIの毎分上限を守る）。
                StepOutcome::Succeeded(_)
                | StepOutcome::AlreadySummarized(_)
                | StepOutcome::Retrying(_) => self.core.wait_interval(ITEM_INTERVAL),
                StepOutcome::Failed(article_id) => {
                    log::warn!("auto summary gave up after {MAX_ATTEMPTS} attempts: {article_id}");
                    self.core.wait_interval(ITEM_INTERVAL)
                }
                StepOutcome::Skipped(_)
                | StepOutcome::Idle
                | StepOutcome::Stopped
                | StepOutcome::Disabled => {}
            }
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

    /// 記事詳細の DTO にキューの状態を反映する。
    pub fn apply_to_detail(&self, detail: &mut ArticleDetailDto) {
        detail.summary_state = self.resolve_state(&detail.article_id, detail.summary_state);
    }

    fn resolve_state(&self, article_id: &str, persisted: SummaryState) -> SummaryState {
        resolve_summary_state(persisted, self.core.state_of(article_id))
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

    /// 設定が読めないときは自動要約しない（外部AIの利用枠を守る安全側）。
    fn is_enabled(&self) -> bool {
        self.load_settings()
            .is_some_and(|settings| is_auto_summary_allowed(&settings))
    }

    fn load_settings(&self) -> Option<PersistedSettings> {
        match self.settings_repository.load_or_default() {
            Ok(settings) => Some(settings),
            Err(error) => {
                log::warn!("failed to load settings for auto summary: {error}");
                None
            }
        }
    }
}

/// 自動要約を動かしてよいか。設定が有効で、AI プロバイダが実装済みの実AIのときだけ true。
/// openai / local は現在実AI呼び出しが未実装で常に Mock 応答になり、毎回失敗するだけなので
/// Mock と同じく動かさない（判断台帳 D56）。未知の値は DTO 変換で Mock 扱いになる。
/// 同梱のローカルLLMプロバイダを実装したら、ここに追加する。
fn is_auto_summary_allowed(settings: &PersistedSettings) -> bool {
    settings.ai.auto_summary_enabled && matches!(settings.to_dto().ai_provider, AiProvider::Gemini)
}

/// 1回の投入件数を、おすすめ順の上位 `maxDailyRecommendations` 件に絞る（常駐負荷を抑えるため）。
/// 上限まで失敗した記事（`failed`）は再起動まで並ばないため、先に除いてから数える
/// （除かないと失敗した記事が枠を埋め、その分ほかの記事が自動要約されなくなる）。
/// 件数と処理間隔は、ローカルLLM導入時に見直す。
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

    #[test]
    fn auto_summary_runs_only_when_enabled_with_a_real_ai_provider() {
        assert!(is_auto_summary_allowed(&settings_with(true, "gemini", 10)));
        assert!(!is_auto_summary_allowed(&settings_with(
            false, "gemini", 10
        )));
        // Mock（未知の値も Mock 扱い）では有効でも動かない。
        assert!(!is_auto_summary_allowed(&settings_with(true, "mock", 10)));
        assert!(!is_auto_summary_allowed(&settings_with(
            true, "unknown", 10
        )));
        // 実AI呼び出しが未実装の openai / local も動かさない。
        assert!(!is_auto_summary_allowed(&settings_with(true, "openai", 10)));
        assert!(!is_auto_summary_allowed(&settings_with(true, "local", 10)));
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
}
