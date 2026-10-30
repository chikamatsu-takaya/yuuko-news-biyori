use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::domain::article::{ArticleDetailDto, ArticleSummaryUpdate, ArticleTagsUpdate};
use crate::domain::settings::{AiProvider, ExplanationLevel};
use crate::domain::speech_tone::SpeechTone;
use crate::domain::summary::{
    AiArticlePoints, AiRequest, AiResponse, GenerateArticleSummaryParams,
    GeneratedArticleSummaryDto, ARTICLE_POINTS_PROMPT_ID, ARTICLE_TAGS_MAX_ITEMS,
    ARTICLE_TAGS_MIN_ITEMS, ARTICLE_TAG_FORBIDDEN_CHARS, ARTICLE_TAG_MAX_CHARS,
    FOCUS_POINTS_MAX_ITEMS, FOCUS_POINTS_MIN_ITEMS, KEY_POINTS_MAX_ITEMS, KEY_POINTS_MIN_ITEMS,
    POINT_ITEM_MAX_CHARS, SUMMARY_INPUT_EXCERPT_LABEL, SUMMARY_INPUT_KEY_POINTS_LABEL,
    SUMMARY_INPUT_TITLE_LABEL, SUMMARY_PROMPT_ID, SUMMARY_RETRY_PROMPT_ID, YUUKO_COMMENT_PROMPT_ID,
    YUUKO_EXPLANATION_PROMPT_ID,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::settings_repository::SettingsRepository;
use crate::util::speech_cleanup::{
    calm_yuuko_speech, casualize_polite_endings, clean_yuuko_speech, count_unlisted_latin_words,
    soften_rude_address, strip_foreign_script_sentences, strip_prompt_echo_sentences,
    strip_repeated_sentences, ForeignScriptOutcome,
};
use crate::util::text_safety::{
    contains_disallowed_control_char, contains_html_tag, neutralize_html_and_control,
};

use super::ai_provider_service::{
    summary_fixed_prompt_texts, yuuko_speech_fixed_prompt_texts, AiProviderService,
};
use consistency_check::{check_stage_consistency, strip_heading_labels};
use leading_connector::strip_leading_connectors;
use output_check::{check_japanese_summary, CheckIssue, CheckReport, IssueKind};

/// 手動の要約（「今すぐ要約」「要約を作成」「作り直す」）を生成中の記事ID。
/// 自動要約キューは、ここにある記事を取り出さず（`AutoSummaryQueue`）、生成中の記事でも段の合間に
/// 手動の要約が始まったら残りの段をやめる（同じ記事の4段生成を二重に走らせないため）。
/// clone しても同じ集合を指す。値は記事IDだけで、本文は持たない。
#[derive(Debug, Clone, Default)]
struct ManualSummaryRegistry {
    /// 記事IDごとの生成中の数（同じ記事の手動要約が重なっても、片方の終わりで外さないため）。
    in_progress: Arc<Mutex<HashMap<String, u32>>>,
}

impl ManualSummaryRegistry {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, u32>> {
        // 中身は単純な数だけなので、他スレッドの panic で poison されてもそのまま使う。
        self.in_progress
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn begin(&self, article_id: &str) -> ManualSummaryGuard {
        *self.lock().entry(article_id.to_string()).or_insert(0) += 1;
        ManualSummaryGuard {
            registry: self.clone(),
            article_id: article_id.to_string(),
        }
    }

    fn contains(&self, article_id: &str) -> bool {
        self.lock().contains_key(article_id)
    }
}

/// 手動の要約の生成中を表す。失敗・panic の道でも、外れたときに集合から外す。
pub(crate) struct ManualSummaryGuard {
    registry: ManualSummaryRegistry,
    article_id: String,
}

impl Drop for ManualSummaryGuard {
    fn drop(&mut self) {
        let mut in_progress = self.registry.lock();
        if let Some(count) = in_progress.get_mut(&self.article_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                in_progress.remove(&self.article_id);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SummaryService {
    ai_provider_service: AiProviderService,
    article_repository: ArticleRepository,
    settings_repository: SettingsRepository,
    manual_summaries: ManualSummaryRegistry,
}

impl SummaryService {
    pub fn new(
        ai_provider_service: AiProviderService,
        article_repository: ArticleRepository,
        settings_repository: SettingsRepository,
    ) -> Self {
        Self {
            ai_provider_service,
            article_repository,
            settings_repository,
            manual_summaries: ManualSummaryRegistry::default(),
        }
    }

    /// 手動の要約。生成中は記事IDを `manual_summaries` に載せ、自動要約キューに同じ記事を生成させない。
    pub fn generate_article_summary(
        &self,
        params: GenerateArticleSummaryParams,
    ) -> Result<GeneratedArticleSummaryDto, AppError> {
        let article_id = params.validated_article_id()?;
        let _manual = self.manual_summaries.begin(&article_id);
        let generation = self.generate_article_summary_with(
            params,
            FallbackPolicy::SaveFallback,
            |request, kind, provider, level| {
                self.request_validated_text(request, kind, provider, level)
            },
        )?;
        match generation {
            SummaryGeneration::Generated(generated) => Ok(generated),
            // 手動生成（SaveFallback）は要約済みでも上書き保存するため、この分岐には来ない。
            SummaryGeneration::AlreadySummarized => Err(AppError::Validation(
                "article summary was not regenerated".to_string(),
            )),
        }
    }

    /// 自動要約キュー用。出力が1つでも Mock（Gemini の失敗・利用枠超過の代替）になったら
    /// 残りの AI 呼び出しをせず、保存せずにエラーを返す（判断台帳 D56）。実AIの出力の検証落ちは
    /// 手動と同じく `AiOutputRejected` で止まる（D104）。記事は未要約のまま残り、
    /// キューの再試行に回る。AI 呼び出し中に手動要約で要約済みになっていた場合は上書きせず
    /// `AutoSummaryOutcome::AlreadySummarized` を返す。
    /// 手動の `generate_article_summary` は Gemini の通信失敗などによる Mock の結果を従来どおり保存し、
    /// 要約済みでも作り直す。
    /// 各段（要点・要約・再説明・感想）の AI 呼び出しの前に、同じ記事の手動要約が始まっていないか・
    /// 要約済みになっていないかを確かめ、そうなら残りの段をやめる（`generate_auto_summary_with`）。
    pub fn generate_article_summary_without_fallback(
        &self,
        params: GenerateArticleSummaryParams,
    ) -> Result<AutoSummaryOutcome, AppError> {
        // ローカルLLMへの要求を「利用者の操作」に数えない（自動要約後の早めの停止を妨げないため）。
        // ローカルLLMでは、この窓口の要求は手動の要求の最中・直後に待たされる（local_llm_service）。
        let background_ai = self.ai_provider_service.for_background();
        self.generate_auto_summary_with(
            params,
            // 手動の要求を待つのは、手を引くかの確認より前（待っている間に手動で要約されることが多いため）。
            |provider| background_ai.wait_for_local_priority(provider),
            |request, kind, provider, level| {
                request_validated_text_via(&background_ai, request, kind, provider, level)
            },
        )
    }

    /// 手動の要約が同じ記事を生成中か（自動要約キューが取り出す前に確かめる）。
    pub fn is_manual_summary_in_progress(&self, article_id: &str) -> bool {
        self.manual_summaries.contains(article_id)
    }

    /// 手動の要約が生成中の状態を作る（テスト用。戻り値を捨てると外れる）。
    #[cfg(test)]
    pub(crate) fn mark_manual_summary_in_progress(&self, article_id: &str) -> ManualSummaryGuard {
        self.manual_summaries.begin(article_id)
    }

    /// 自動要約の本体。各段の前に `wait_before_stage`（ローカルLLMでは手動の要求の優先待ち）を済ませてから、
    /// 手動要約との重なりを確かめる。手動要約が生成中なら `YieldedToManual`、要約済みになっていれば
    /// `AlreadySummarized` を返し、残りの AI 呼び出しも保存もしない（どちらも失敗には数えない）。
    /// 待った後に確かめるので、待っている間に利用者が同じ記事を要約しても、その段（20〜60 秒）を無駄に生成しない。
    fn generate_auto_summary_with<W, F>(
        &self,
        params: GenerateArticleSummaryParams,
        wait_before_stage: W,
        request_validated: F,
    ) -> Result<AutoSummaryOutcome, AppError>
    where
        W: Fn(AiProvider) -> bool,
        F: Fn(
            AiRequest,
            SummaryOutputKind,
            AiProvider,
            ExplanationLevel,
        ) -> Result<AiResponse, AppError>,
    {
        let article_id = params.validated_article_id()?;
        let stopped_by = Cell::new(None);
        let generation = self.generate_article_summary_with(
            params,
            FallbackPolicy::RejectFallback,
            |request, kind, provider, level| {
                if !wait_before_stage(provider) {
                    // アプリ終了で待ちを打ち切った。失敗として返す（終了中なので再試行はされない）。
                    return Err(AppError::Validation(
                        "auto summary stopped because the app is exiting".to_string(),
                    ));
                }
                if let Some(outcome) = self.auto_summary_preempted_by(&article_id) {
                    stopped_by.set(Some(outcome));
                    return Err(AppError::Validation(
                        "auto summary stopped because the article was summarized manually"
                            .to_string(),
                    ));
                }
                request_validated(request, kind, provider, level)
            },
        );
        if let Some(outcome) = stopped_by.get() {
            log::info!("auto summary for {article_id} stopped before {outcome:?}");
            return Ok(outcome);
        }
        Ok(match generation? {
            SummaryGeneration::Generated(_) => AutoSummaryOutcome::Saved,
            SummaryGeneration::AlreadySummarized => AutoSummaryOutcome::AlreadySummarized,
        })
    }

    /// 自動要約の次の段へ進む前に、やめるべきか。手動要約が生成中なら `YieldedToManual`、
    /// 記事ファイルで要約済みなら `AlreadySummarized`。読めないときは続ける（保存直前の確認が守る）。
    fn auto_summary_preempted_by(&self, article_id: &str) -> Option<AutoSummaryOutcome> {
        if self.manual_summaries.contains(article_id) {
            return Some(AutoSummaryOutcome::YieldedToManual);
        }
        match self.article_repository.is_article_summarized(article_id) {
            Ok(true) => Some(AutoSummaryOutcome::AlreadySummarized),
            Ok(false) | Err(_) => None,
        }
    }

    /// 要約生成の本体。検証済み出力の取得手段（`request_validated`）を差し替えられるようにし、
    /// 検証失敗時に既存保存値が残ることをテストで確認できるようにする。
    fn generate_article_summary_with<F>(
        &self,
        params: GenerateArticleSummaryParams,
        fallback_policy: FallbackPolicy,
        request_validated: F,
    ) -> Result<SummaryGeneration, AppError>
    where
        F: Fn(
            AiRequest,
            SummaryOutputKind,
            AiProvider,
            ExplanationLevel,
        ) -> Result<AiResponse, AppError>,
    {
        let article_id = params.validated_article_id()?;
        let article = self.article_repository.get_article_detail(&article_id)?;
        let settings = self.settings_repository.load_or_default()?;
        let provider = settings.to_dto().ai_provider;
        let explanation_level = ExplanationLevel::from_storage(&settings.explanation.level);

        // 種（Mock 結果そのもの・実AIへの入力）は、外部由来の値を無害化した記事から作る。
        // これにより Mock 結果が出力検証に落ちないこと（＝AIキー未設定でも要約できること）を構造的に保証する。
        // 種の中の注目ポイント（seed_focus_points）は要約の種・再説明の Mock 文のための材料で、保存はしない。
        // 保存する要点・注目ポイント・タグは、下の article_points_v2 の AI 出力から取る（D18 / D11）。
        let seed_article = neutralize_seed_article(&article);
        let seed_focus_points = build_focus_points(&seed_article, explanation_level);
        // 要約の Mock の結果の種（本文抜粋＋定型文）。D167 の前の要約の入力と同じで、Mock 専用（context で渡し、
        // 実AIへは送らない）。Mock の要約の文面を変えないため。
        let summary_mock_seed =
            build_summary_seed(&seed_article, explanation_level, &seed_focus_points);
        let points_seed = build_points_seed(&seed_article);
        // 機械点検（D166・D170）の材料: 要点の種（タイトル＋本文抜粋）。記事の公開日は足さない
        // （取得元に公開日が無いときは取得時刻が入り〔`normalize_published_at`〕、本当の記事の日付と区別できないため）。
        let check_source = points_seed.clone();
        // 実AIの要約・再説明・一言は、点検（D166・D170）の前に文頭のつなぎの語（「これにより、」「その結果、」）を消す
        // （`strip_ai_leading_connectors`）。作り直しも同じ取得手段を通すので、点検・保存・再説明の入力は消した後の文になる。
        let request_validated = |request: AiRequest,
                                 kind: SummaryOutputKind,
                                 provider: AiProvider,
                                 level: ExplanationLevel| {
            request_validated(request, kind, provider, level)
                .map(|response| strip_ai_leading_connectors(kind, response, &check_source))
        };

        // 4出力（要点と注目ポイント・要約・再説明・感想）とも詳細設計書 §12.5 の出力検証を通ったものだけを
        // 保存・返却する。
        // どれか1つでも（Mock を含めて）有効な出力を得られなければ、保存せず固定文言のエラーを返す。
        // その場合、記事Markdownに保存済みの要約・再説明・感想は上書きされずに残る。
        // 自動要約（RejectFallback）では、Mock に切り替わった時点で残りの AI 呼び出しをせずに止める
        // （キー未設定・利用枠超過のときに、保存しない出力のために外部AIを呼び続けないため）。
        // 順序は 要点 → 要約 → 再説明 → 感想（D167）。要約は先に作った要点（事実の箇条書き）を軸に組み立てさせる
        // （本文抜粋だけから要約させると、小さいモデルが中心の出来事を落としたり主語と目的語を取り違えたりした一方、
        // 要点は正確だったため）。再説明・感想の入力には前の2つの出力を使う。生成の回数は変わらない。
        // どの段で拒否・失敗しても `?` で止まるので、後の呼び出しはしない（D104 / D56）。

        // 要点・注目ポイント・タグ（D18 / D11）。ローカルLLMの負荷を抑えるため、1回の呼び出しで受け取る
        // （タグのためだけに AI を呼ばない）。
        // 入力は無害化したタイトル＋本文抜粋だけにする（既存の注目ポイントや定型文を混ぜない）。
        // context の無害化済みジャンルは Mock のタグを作るためだけに使い、実AIへは送らない
        // （ai_provider_service の build_prompt は用語解説以外で context を使わない）。
        let points_request = AiRequest {
            prompt_id: ARTICLE_POINTS_PROMPT_ID.to_string(),
            input_text: points_seed.clone(),
            context: Some(seed_article.genre.clone()),
        };
        let points_response = request_validated(
            points_request.clone(),
            SummaryOutputKind::Points,
            provider,
            explanation_level,
        )?;
        reject_fallback_early(fallback_policy, &article_id, &points_response)?;
        // 取得手段に関わらず、保存前にもう一度ここで検証して構造化する（形の崩れた出力を保存しない）。
        let points = article_points_from_response(&points_response)?;
        // 要点・注目ポイント・タグの点検（D170）。材料と反対の意味・材料にない年の項目は外し、外して件数が
        // 足りなくなったときだけ1回作り直す。
        let (points_response, points) = check_and_retry_points(
            points_response,
            points,
            points_request,
            &check_source,
            provider,
            explanation_level,
            fallback_policy,
            &request_validated,
        )?;

        // 要約（D167）の実AI入力: 無害化したタイトル・本文抜粋と、いま作った要点。要点が Mock の代替（手動生成で
        // 要点だけ Gemini が失敗したとき。本文の文と定型文）なら入れない（実AIの出力ではない文を実AIの入力に混ぜない）。
        let summary_key_points =
            (points_response.provider != PROVIDER_MOCK).then_some(points.key_points.as_slice());
        let summary_input = build_summary_input(
            &seed_article.title,
            seed_article.excerpt.as_deref(),
            summary_key_points,
        );
        let summary_response = request_validated(
            AiRequest {
                prompt_id: SUMMARY_PROMPT_ID.to_string(),
                input_text: summary_input.clone(),
                context: Some(summary_mock_seed),
            },
            SummaryOutputKind::Summary,
            provider,
            explanation_level,
        )?;
        reject_fallback_early(fallback_policy, &article_id, &summary_response)?;
        // 実AIの要約は機械点検にかけ、問題があれば理由を添えて1回だけ作り直す（D166）。
        // 作り直しも同じ取得手段（自動要約では手動要約との重なりの確認を含む）を通す。
        // 点検の材料は記事そのもの（無害化したタイトル＋本文抜粋＝要点の種）にし、要点は含めない
        // （要点も AI の出力なので、要点にだけある数字を要約が写したときも「記事情報にない数字」として拾うため）。
        // 作り直しの入力からは要点を外す（点検の指摘が要点の誤りを写したことによるときに、同じ要点から
        // 作り直して同じ誤りを繰り返さないため）。指示も要点に触れない版になる（ai_provider_service）。
        let retry_input =
            build_summary_input(&seed_article.title, seed_article.excerpt.as_deref(), None);
        let summary_response = check_and_retry_summary(
            summary_response,
            &retry_input,
            &check_source,
            provider,
            explanation_level,
            fallback_policy,
            &request_validated,
        )?;

        // 再説明・一言の実AI入力: 同じ生成で得た要約・要点（検証済み）。定型文の言い換えにさせないため、
        // 記事の中身を渡す。AI 出力も外部データ由来なので、種と同じく無害化・切り詰めてから渡す
        // （プロンプト側でも「記事情報（外部データ）」の見出しの下に置く）。
        // 要約が Mock の代替（手動生成で要約だけ Gemini が失敗したとき）は、本文の短い抜粋（実AIの出力ではない）なので入力に入れない。
        let speech_summary =
            (summary_response.provider != PROVIDER_MOCK).then_some(summary_response.text.as_str());
        let yuuko_speech_input =
            build_yuuko_speech_input(&seed_article.title, speech_summary, &points.key_points);
        // 再説明・一言の定型文の種は Mock の結果専用（context で渡し、実AIへは送らない）。
        // 重いニュース（D122）では落ち着いた定型文にする。判定は実AIのプロンプト・後処理と同じく、
        // 再説明・一言の入力（タイトル・要約・要点）から行う（`SpeechTone::for_request` と同じ判定）。
        let speech_tone = SpeechTone::detect(&yuuko_speech_input);
        let yuuko_explanation_mock_text = build_yuuko_explanation_seed(
            &seed_article,
            explanation_level,
            &seed_focus_points,
            speech_tone,
        );
        let yuuko_comment_mock_text =
            build_yuuko_comment_seed(&seed_article, explanation_level, speech_tone);
        // 再説明・一言も点検し（D170。材料は要約と同じ記事そのもの）、問題があれば1回だけ作り直す。
        // 作り直しても残ったら、定型文（Mock の結果と同じ文）を使う（`check_and_retry_speech`）。
        let yuuko_explanation_request = AiRequest {
            prompt_id: YUUKO_EXPLANATION_PROMPT_ID.to_string(),
            input_text: yuuko_speech_input.clone(),
            context: Some(yuuko_explanation_mock_text),
        };
        let yuuko_explanation_response = request_validated(
            yuuko_explanation_request.clone(),
            SummaryOutputKind::Explanation,
            provider,
            explanation_level,
        )?;
        reject_fallback_early(fallback_policy, &article_id, &yuuko_explanation_response)?;
        let yuuko_explanation_response = check_and_retry_speech(
            yuuko_explanation_response,
            yuuko_explanation_request,
            SummaryOutputKind::Explanation,
            &check_source,
            provider,
            explanation_level,
            fallback_policy,
            &request_validated,
        )?;

        let yuuko_comment_request = AiRequest {
            prompt_id: YUUKO_COMMENT_PROMPT_ID.to_string(),
            input_text: yuuko_speech_input,
            context: Some(yuuko_comment_mock_text),
        };
        let yuuko_comment_response = request_validated(
            yuuko_comment_request.clone(),
            SummaryOutputKind::Comment,
            provider,
            explanation_level,
        )?;
        reject_fallback_early(fallback_policy, &article_id, &yuuko_comment_response)?;
        let yuuko_comment_response = check_and_retry_speech(
            yuuko_comment_response,
            yuuko_comment_request,
            SummaryOutputKind::Comment,
            &check_source,
            provider,
            explanation_level,
            fallback_policy,
            &request_validated,
        )?;

        // 永続化メタ用のプロバイダ。1つでも Mock に切り替わっていれば "mock" と記録する。
        let effective_provider = combined_provider(&[
            &summary_response,
            &yuuko_explanation_response,
            &yuuko_comment_response,
            &points_response,
        ]);
        let summary = summary_response.text;
        let yuuko_explanation = yuuko_explanation_response.text;
        let yuuko_comment = yuuko_comment_response.text;
        let AiArticlePoints {
            key_points,
            focus_points,
            tags,
        } = points;
        // タグは検証を通ったとき（3〜5件）だけ反映し、落ちたとき（空）は既存のタグを残す。
        // タグの失敗では要約の保存を止めない（D11。要点・注目ポイントの検証落ちとは扱いが違う）。
        // Mock（provider=Mock・Gemini 失敗時の代替）のタグはジャンルからの定型なので、既存のタグを
        // 上書きせず、空のときだけ入れる（安全側。実AIの有効なタグは置き換える）。
        let tags = if tags.is_empty() {
            ArticleTagsUpdate::Keep
        } else if points_response.provider == PROVIDER_MOCK {
            ArticleTagsUpdate::FillIfEmpty(tags)
        } else {
            ArticleTagsUpdate::Replace(tags)
        };
        let update = ArticleSummaryUpdate {
            summary: summary.clone(),
            yuuko_explanation: yuuko_explanation.clone(),
            key_points: key_points.clone(),
            focus_points: focus_points.clone(),
            tags,
            yuuko_comment: yuuko_comment.clone(),
            ai_provider: effective_provider,
            generated_at: current_utc_timestamp(),
        };

        match fallback_policy {
            FallbackPolicy::SaveFallback => {
                // B-4: 生成要約を記事Markdownへ永続化（再表示はキャッシュ・更新は明示再生成）。
                // 保存失敗でもアプリは止めず、生成結果は返す（警告ログのみ）。CLAUDE.md §10「安全側へ倒す」。
                let persist_result = self
                    .article_repository
                    .update_article_summary(&article_id, update);
                if let Err(error) = persist_result {
                    log::warn!("failed to persist generated summary for {article_id}: {error}");
                }
            }
            FallbackPolicy::RejectFallback => {
                // 自動要約は、AI 呼び出し中に手動要約で要約済みになっていたら上書きしない。
                // 確認と保存は同じ書き込みロック内で行う。保存失敗は返して、キューの再試行に回す。
                let saved = self
                    .article_repository
                    .update_article_summary_if_unsummarized(&article_id, update)?;
                if !saved {
                    log::info!(
                        "auto summary for {article_id} was not saved because it was already summarized"
                    );
                    return Ok(SummaryGeneration::AlreadySummarized);
                }
            }
        }

        Ok(SummaryGeneration::Generated(GeneratedArticleSummaryDto {
            article_id,
            summary,
            yuuko_explanation,
            key_points,
            focus_points,
            yuuko_comment,
        }))
    }

    /// AI 出力を取得し、§12.5 の出力検証を通した結果だけを返す。
    /// 実AI（Gemini・ローカル）の出力が検証に落ちたときは、手動・自動とも Mock の結果へ切り替えず、
    /// 保存させないためにエラーを返す（判断台帳 D104。`select_valid_output`）。
    fn request_validated_text(
        &self,
        request: AiRequest,
        kind: SummaryOutputKind,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        request_validated_text_via(
            &self.ai_provider_service,
            request,
            kind,
            provider,
            explanation_level,
        )
    }

    /// 実AIを呼べる見込みがあるかの安い確認（自動要約キューの可否判定用）。AI は呼ばない。
    pub fn is_real_ai_ready(&self, provider: AiProvider) -> bool {
        self.ai_provider_service.is_real_ai_ready(provider)
    }
}

/// 要約の機械点検（`output_check`）と1回の作り直し（判断台帳 D166）。
///
/// - 点検するのは実AI（Gemini・ローカル）の要約だけ。Mock の結果（本文の抜粋）は点検しない。
/// - 作り直すのは、国内記事の評価で誤検出の少なかった種類（`IssueKind::triggers_summary_retry`）が
///   見つかったときだけ。理由（固定の文と数字・字だけ）を添えて、`retry_input` から1回だけ作り直す。
///   `retry_input` は要点を外した入力（タイトル＋本文抜粋。D167）で、要点の誤りを写した要約を、同じ要点から
///   作り直さないようにする。
/// - 点検の材料（`check_source`）は、AI へ渡した入力とは別に渡す（要約は要点を含む入力から作るが、点検は
///   記事そのもの〔タイトル＋本文抜粋〕と照らす。D167）。
/// - 作り直した要約を採るのは、作り直しの理由になる問題の数が減り、新しい種類の問題が出ていないときだけ。
///   作り直しが失敗した・検証に落ちた・Mock に切り替わった・良くならなかったときは、最初の要約を使う
///   （最初の要約は保存前の検証を通っているので、作り直しで悪化させない）。
/// - 自動要約で、作り直しの前に手動要約が始まった・アプリが終わるときは、取得手段が停止のエラー
///   （`AppError::Validation`）を返す。握りつぶさずにそのまま返し、自動要約を止める（`generate_auto_summary_with`）。
/// - 自動要約（RejectFallback）で作り直しが Mock に切り替わったときは、その時点でエラーを返して止める
///   （`reject_fallback_early`。最初の段と同じく、Mock になったら残りの AI 呼び出しをしない）。手動は最初の要約を使う。
/// - ログは INFO で、問題の種類（固定ラベル）と結果だけを残す（本文・URL・理由の文は出さない）。
fn check_and_retry_summary<F>(
    first: AiResponse,
    retry_input: &str,
    check_source: &str,
    provider: AiProvider,
    explanation_level: ExplanationLevel,
    fallback_policy: FallbackPolicy,
    request_validated: &F,
) -> Result<AiResponse, AppError>
where
    F: Fn(
        AiRequest,
        SummaryOutputKind,
        AiProvider,
        ExplanationLevel,
    ) -> Result<AiResponse, AppError>,
{
    if first.provider == PROVIDER_MOCK {
        return Ok(first);
    }
    let fixed_texts = summary_fixed_prompt_texts();
    let fixed_refs: Vec<&str> = fixed_texts.iter().map(String::as_str).collect();
    let first_report = check_japanese_summary(check_source, &first.text, None, &fixed_refs);
    let first_kinds = first_report.kinds_where(IssueKind::triggers_summary_retry);
    if first_kinds.is_empty() {
        return Ok(first);
    }
    let reasons = retry_reasons(&first_report);
    log::info!(
        "the summary output check found issues ({}); regenerating the summary once",
        issue_labels(&first_kinds)
    );
    let retried = request_validated(
        AiRequest {
            prompt_id: SUMMARY_RETRY_PROMPT_ID.to_string(),
            input_text: retry_input.to_string(),
            context: Some(reasons),
        },
        SummaryOutputKind::Summary,
        provider,
        explanation_level,
    );
    let retried = match retried {
        Ok(retried) if retried.provider != PROVIDER_MOCK => retried,
        Ok(retried) => {
            // 自動要約はここで止める（記事は未要約のまま・キューの再試行に回る）。手動は最初の要約を使う。
            reject_fallback_early_with_label(fallback_policy, "summary regeneration", &retried)?;
            log::info!(
                "the summary regeneration fell back to the mock provider; kept the first summary"
            );
            return keep_first_unless_blocked(first, &first_report);
        }
        // 手動要約との重なり・アプリの終了による停止（自動要約の取得手段が返す）は、握りつぶさずに返す。
        Err(error @ AppError::Validation(_)) => {
            log::info!("the summary regeneration was stopped; the summary was not regenerated");
            return Err(error);
        }
        Err(error) => {
            log::info!(
                "the summary regeneration failed ({}); kept the first summary",
                error_label(&error)
            );
            return keep_first_unless_blocked(first, &first_report);
        }
    };
    let retried_report = check_japanese_summary(check_source, &retried.text, None, &fixed_refs);
    let retried_kinds = retried_report.kinds_where(IssueKind::triggers_summary_retry);
    // 反対の意味・年の取り違え（D170）が作り直しで消えたら、ほかの問題が残っていても作り直した方を使う
    // （意味を逆にした要約を見せるより、数字の言い換えなどが残る方が害が小さいため）。
    // 作り直しに反対の意味・年の取り違えが残るときは、数が減っていても採らない（D170）。
    if !retried_report.blocks_use()
        && (is_retry_better(&first_report, &retried_report) || first_report.blocks_use())
    {
        log::info!(
            "adopted the regenerated summary (remaining issues: {})",
            issue_labels(&retried_kinds)
        );
        Ok(retried)
    } else {
        log::info!(
            "kept the first summary because the regenerated one was not better ({})",
            issue_labels(&retried_kinds)
        );
        keep_first_unless_blocked(first, &first_report)
    }
}

/// 作り直しを使わないときに、最初の要約を使ってよいか。反対の意味・年の取り違え（`IssueKind::blocks_use`）が
/// 残る要約は使わず、実AIの出力が検証に落ちたときと同じく保存しない（D170・D104。手動は固定文言のエラー、
/// 自動要約はキューの再試行に回る）。
fn keep_first_unless_blocked(
    first: AiResponse,
    first_report: &CheckReport,
) -> Result<AiResponse, AppError> {
    if first_report.blocks_use() {
        log::warn!(
            "the summary still contradicted the article after the regeneration ({}); the generated summary was not saved",
            issue_labels(&first_report.kinds_where(IssueKind::blocks_use))
        );
        return Err(AppError::AiOutputRejected);
    }
    Ok(first)
}

/// 要点・注目ポイント・タグの点検（D170）と、件数が足りなくなったときの1回の作り直し。
///
/// - 点検するのは実AIの出力だけ（Mock の要点は本文の文なので点検しない）。
/// - 各項目から見出し・太字の記号を消し（`strip_heading_labels`）、材料（タイトル＋本文抜粋）と反対の意味・
///   材料にない年・年の組み合わせの誤り・推測の言い切り（D175）・消せない見出しのある項目は外す（`check_stage_consistency`）。
///   タグは反対の意味のものだけを外し、3件に満たなくなったらタグを空にする（既存のタグを残す・D11）。
/// - 外しても要点 2件・注目ポイント 1件以上が残れば、作り直さずに残りを使う（生成の回数を増やさない）。
/// - 足りなくなったときだけ、同じ依頼で1回作り直す（要点の段には点検の理由を添える指示がないため）。
///   作り直しも点検し、足りなければ実AIの出力が検証に落ちたときと同じく保存しない（D104）。
/// - 作り直しが Mock に切り替わったとき: 自動要約は止め、手動は Mock の要点（本文の文）を使う。
/// - 自動要約で、作り直しの前に手動要約が始まった・アプリが終わるときの停止のエラーはそのまま返す。
#[allow(clippy::too_many_arguments)]
fn check_and_retry_points<F>(
    first_response: AiResponse,
    first: AiArticlePoints,
    request: AiRequest,
    source: &str,
    provider: AiProvider,
    explanation_level: ExplanationLevel,
    fallback_policy: FallbackPolicy,
    request_validated: &F,
) -> Result<(AiResponse, AiArticlePoints), AppError>
where
    F: Fn(
        AiRequest,
        SummaryOutputKind,
        AiProvider,
        ExplanationLevel,
    ) -> Result<AiResponse, AppError>,
{
    if first_response.provider == PROVIDER_MOCK {
        return Ok((first_response, first));
    }
    let first_kinds = match filter_checked_points(first, source) {
        Ok(points) => return Ok((first_response, points)),
        Err(kinds) => kinds,
    };
    log::info!(
        "the article points check left too few items ({}); regenerating the points once",
        issue_labels(&first_kinds)
    );
    let retried = match request_validated(
        request,
        SummaryOutputKind::Points,
        provider,
        explanation_level,
    ) {
        Ok(retried) => retried,
        Err(error @ AppError::Validation(_)) => return Err(error),
        Err(error) => {
            log::warn!(
                "the article points regeneration failed ({}); the generated summary was not saved",
                error_label(&error)
            );
            return Err(AppError::AiOutputRejected);
        }
    };
    let points = article_points_from_response(&retried)?;
    if retried.provider == PROVIDER_MOCK {
        reject_fallback_early_with_label(fallback_policy, "article points regeneration", &retried)?;
        return Ok((retried, points));
    }
    match filter_checked_points(points, source) {
        Ok(points) => {
            log::info!(
                "adopted the regenerated article points (first issues: {})",
                issue_labels(&first_kinds)
            );
            Ok((retried, points))
        }
        Err(kinds) => {
            log::warn!(
                "the regenerated article points still left too few items ({}); the generated summary was not saved",
                issue_labels(&kinds)
            );
            Err(AppError::AiOutputRejected)
        }
    }
}

/// 要点・注目ポイント・タグを点検し、問題のある項目を外す。件数が足りなくなったら、外した理由の種類を返す。
fn filter_checked_points(
    points: AiArticlePoints,
    source: &str,
) -> Result<AiArticlePoints, Vec<IssueKind>> {
    let mut kinds: Vec<IssueKind> = Vec::new();
    let mut keep_item = |item: String| -> Option<String> {
        let (item, _) = strip_heading_labels(&item);
        let issues = check_stage_consistency(source, &item);
        if issues.is_empty() && !item.trim().is_empty() {
            return Some(item);
        }
        kinds.extend(issues.iter().map(|issue| issue.kind));
        None
    };
    let key_points: Vec<String> = points
        .key_points
        .into_iter()
        .filter_map(&mut keep_item)
        .collect();
    let focus_points: Vec<String> = points
        .focus_points
        .into_iter()
        .filter_map(&mut keep_item)
        .collect();
    let tag_count = points.tags.len();
    let mut tags: Vec<String> = points
        .tags
        .into_iter()
        .filter(|tag| consistency_check::find_contradictions(source, tag).is_empty())
        .collect();
    if tags.len() < tag_count {
        kinds.push(IssueKind::Contradiction);
        if tags.len() < ARTICLE_TAGS_MIN_ITEMS {
            tags.clear();
        }
    }
    kinds.sort_unstable();
    kinds.dedup();
    if !kinds.is_empty() {
        // 本文は出さず、外した理由の種類だけを残す。
        log::info!(
            "removed article point items or tags that failed the check ({})",
            issue_labels(&kinds)
        );
    }
    if key_points.len() < KEY_POINTS_MIN_ITEMS || focus_points.len() < FOCUS_POINTS_MIN_ITEMS {
        return Err(kinds);
    }
    Ok(AiArticlePoints {
        key_points,
        focus_points,
        tags,
    })
}

/// 再説明・一言の点検（D170）と1回の作り直し。
///
/// - 点検するのは実AIの出力だけ。材料は記事そのもの（タイトル＋本文抜粋）で、見出しの残り・反対の意味・
///   材料にない年・年の組み合わせ・推測の言い切り（D175）を見る（`check_stage_consistency`）。問題がなければ作り直さない。
/// - 問題があれば同じ依頼で1回だけ作り直す（再説明・一言には点検の理由を添える指示がないため。
///   temperature 0.2 の生成のばらつきで、別の言い方になることを期待する）。
/// - 作り直しも問題が残る・失敗した・検証に落ちたときは、定型文（`request.context`。Mock の結果と同じ、
///   タイトルとジャンルからの決まった文）を使う。誤った内容を見せるより安全なため。プロバイダの記録は
///   変えない（要約・要点は実AIの出力のまま）。
/// - 自動要約で、作り直しの前に手動要約が始まった・アプリが終わるときの停止のエラーはそのまま返す。
///   作り直しが Mock に切り替わったときは、自動要約は止め、手動は Mock の結果（同じ定型文）を使う。
#[allow(clippy::too_many_arguments)]
fn check_and_retry_speech<F>(
    first: AiResponse,
    request: AiRequest,
    kind: SummaryOutputKind,
    source: &str,
    provider: AiProvider,
    explanation_level: ExplanationLevel,
    fallback_policy: FallbackPolicy,
    request_validated: &F,
) -> Result<AiResponse, AppError>
where
    F: Fn(
        AiRequest,
        SummaryOutputKind,
        AiProvider,
        ExplanationLevel,
    ) -> Result<AiResponse, AppError>,
{
    if first.provider == PROVIDER_MOCK {
        return Ok(first);
    }
    let first_issues = check_stage_consistency(source, &first.text);
    if first_issues.is_empty() {
        return Ok(first);
    }
    log::info!(
        "the {} check found issues ({}); regenerating it once",
        kind.label(),
        issue_labels(&issue_kinds(&first_issues))
    );
    let template = request.context.clone().unwrap_or_default();
    let retried = request_validated(request, kind, provider, explanation_level);
    let retried_issues = match retried {
        Ok(retried) if retried.provider == PROVIDER_MOCK => {
            reject_fallback_early_with_label(fallback_policy, kind.label(), &retried)?;
            return Ok(retried);
        }
        Ok(retried) => {
            let issues = check_stage_consistency(source, &retried.text);
            if issues.is_empty() {
                log::info!("adopted the regenerated {}", kind.label());
                return Ok(retried);
            }
            issue_labels(&issue_kinds(&issues))
        }
        Err(error @ AppError::Validation(_)) => return Err(error),
        Err(error) => error_label(&error).to_string(),
    };
    log::info!(
        "the regenerated {} was not usable ({retried_issues}); used the fixed text instead",
        kind.label()
    );
    // 定型文は Mock の種と同じく無害化済みで、保存前の検証を通る。念のため同じ検証にかける。
    let text = validate_summary_output(kind, &template).map_err(|_| AppError::AiOutputRejected)?;
    Ok(AiResponse {
        text,
        provider: first.provider,
    })
}

fn issue_kinds(issues: &[CheckIssue]) -> Vec<IssueKind> {
    let mut kinds: Vec<IssueKind> = issues.iter().map(|issue| issue.kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
}

/// 自動要約で作り直しが Mock に切り替わったときのエラー（`reject_fallback_early` と同じエラー）。
/// ログには記事IDの代わりに段の名前だけを出す（この関数は記事IDを持たない）。
fn reject_fallback_early_with_label(
    fallback_policy: FallbackPolicy,
    stage: &str,
    response: &AiResponse,
) -> Result<(), AppError> {
    if fallback_policy == FallbackPolicy::RejectFallback && response.provider == PROVIDER_MOCK {
        log::warn!(
            "auto summary {stage} fell back to the mock provider; the remaining AI calls were skipped and nothing was saved"
        );
        return Err(AppError::Network(
            "real AI output was unavailable; fallback output was not saved".to_string(),
        ));
    }
    Ok(())
}

/// 作り直しのプロンプトに添える理由（1行1件・重複なし）。作り直しの理由になる種類だけを入れる。
fn retry_reasons(report: &CheckReport) -> String {
    let mut reasons: Vec<&str> = Vec::new();
    for issue in &report.issues {
        if issue.kind.triggers_summary_retry() && !reasons.contains(&issue.reason.as_str()) {
            reasons.push(&issue.reason);
        }
    }
    reasons.join(
        "
",
    )
}

/// 作り直した要約が最初の要約より良いか: 作り直しの理由になる問題の数が減り、最初になかった種類が出ていない。
fn is_retry_better(first: &CheckReport, retried: &CheckReport) -> bool {
    let count = |report: &CheckReport| {
        report
            .issues
            .iter()
            .filter(|issue| issue.kind.triggers_summary_retry())
            .count()
    };
    let first_kinds = first.kinds_where(IssueKind::triggers_summary_retry);
    count(retried) < count(first)
        && retried
            .kinds_where(IssueKind::triggers_summary_retry)
            .iter()
            .all(|kind| first_kinds.contains(kind))
}

fn issue_labels(kinds: &[IssueKind]) -> String {
    if kinds.is_empty() {
        return "none".to_string();
    }
    kinds
        .iter()
        .map(|kind| kind.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// ログ用のエラーの固定ラベル（本文・理由の文を出さない）。
fn error_label(error: &AppError) -> &'static str {
    match error {
        AppError::AiOutputRejected => "output_rejected",
        AppError::LocalAi(_) => "local_ai_failed",
        _ => "other",
    }
}

/// AI 出力を指定の窓口（手動用・自動要約用）で取得し、§12.5 の出力検証を通した結果だけを返す。
fn request_validated_text_via(
    ai_provider_service: &AiProviderService,
    request: AiRequest,
    kind: SummaryOutputKind,
    provider: AiProvider,
    explanation_level: ExplanationLevel,
) -> Result<AiResponse, AppError> {
    // 口調はプロンプト（ai_provider_service の build_prompt）と同じ入力から判定する（D122）。
    let tone = SpeechTone::for_request(&request.prompt_id, &request.input_text);
    // 再説明・一言の後処理（中国語の字・英単語の混入の判定）で、入力にある字・語を除くために使う。
    let input_text = request.input_text.clone();
    let primary = ai_provider_service.request_text(request, provider, explanation_level)?;
    select_valid_output_in_tone(kind, tone, &input_text, primary)
}

// --- AI出力検証（詳細設計書 §12.5）: 副作用なしの純粋 helper ---
//
// §12.5 は数値上限を定めていないため、用語解説側（dictionary_service の SHORT 300 / DETAIL 2000）に
// 揃えて定数化し、値だけで調整できるようにする。超過時は切り詰めず拒否する（途中で切れた文を
// 保存しない・用語解説側と同じ扱い）。

/// AI要約の上限。AI へは「1〜2文」で依頼するため実AIの正常出力はこれより十分短い。
/// Mock 結果は本文抜粋に定型文・注目ポイントを足した種そのもの。種の抜粋は SEED_EXCERPT_MAX_CHARS
/// （1500文字）で切り詰めるため、Mock 結果が必ずこの上限に収まるよう 3000 文字とする。
const SUMMARY_MAX_CHARS: usize = 3_000;
/// ゆうこの再説明（「ゆうこの用語解説」セクション）の上限。用語解説の detail 上限（2000文字）に揃える。
const EXPLANATION_MAX_CHARS: usize = 2_000;
/// ゆうこの感想（「ゆうこの一言」セクション）の上限。「一言」なので用語解説の short 上限（300文字）に揃える。
const COMMENT_MAX_CHARS: usize = 300;

/// AiProviderService が Mock 応答に付けるプロバイダ名（保存メタ ai_provider と同じ値）。
const PROVIDER_MOCK: &str = "mock";

// --- 種（Mock 結果・AI入力）の無害化上限 ---
// Mock 結果は種そのもの（要約は種の先頭の短い抜粋）なので、種の各部品をここで切り詰めておけば各出力上限に必ず収まる。
// 要約の種 = 抜粋(1500) + 注目ポイント2件(各 タイトル200＋定型文 程度) + 定型文(約70) ≦ 3000。
// 感想の種 = タイトル(200) または ジャンル(100) + 定型文(約60) ≦ 300。
// 要点・注目ポイントの種 = タイトル(200) + 改行 + 抜粋(1500)。Mock は種の文から各件 100 文字以内で組む。
// 再説明の種 = ジャンル(100) + 注目ポイント2件(各 300 以下) + 定型文(約110) ≦ 2000。
/// 種に使う本文抜粋の上限。RSS description 経由の抜粋は取得側で切り詰められないため、ここで抑える。
/// 1500 文字にしたのは、実ニュース30本の比較（判断台帳 D126）で、2300 文字より長い記事の要約が正確になり、
/// 生成の失敗も減り速くなったため（小さいローカルLLMは長い入力で主語・向きを取り違えやすい）。
const SEED_EXCERPT_MAX_CHARS: usize = 1_500;
/// 種に使うタイトルの上限。
const SEED_TITLE_MAX_CHARS: usize = 200;
/// 種に使うジャンルの上限。
const SEED_GENRE_MAX_CHARS: usize = 100;
/// 種に使う注目ポイント1件の上限。
const SEED_FOCUS_POINT_MAX_CHARS: usize = 200;
/// 再説明・一言の実AI入力に使う要約の上限。実AIの要約は1〜2文（百数十文字）なので通常は切れない。
/// Mock の要約（種の先頭の短い抜粋）や長い出力が混ざったときでも、入力が抜粋ほど長くならないように抑える。
const SEED_SUMMARY_MAX_CHARS: usize = 400;

/// 種へ入れる外部由来文字列（抜粋・タイトル・ジャンル・注目ポイント）を、出力検証に落ちない形へ無害化する。
///
/// 順序: 文字数で切り詰め → 制御文字（改行・タブ以外）を除去 → 行ごとに `<` を全角 `＜` へ、
/// 行頭（先頭空白の後）の `#` を全角 `＃` へ置換し、区切り行を落とす。
/// 切り詰めを先に行うのは、途中で切れて新たに `---` だけの行ができる、といったことを防ぐため
/// （後段の処理は文字数を増やさない）。制御文字の除去を行処理より先に行うのは、
/// 除去によって行頭に `#` が現れるケースを取りこぼさないため。
fn neutralize_seed_text(text: &str, max_chars: usize) -> String {
    let truncated = text.chars().take(max_chars).collect::<String>();
    // 制御文字の除去と `<` の置換は用語解説 Mock と共通（util::text_safety）。
    neutralize_html_and_control(&truncated)
        .split('\n')
        .filter(|line| !is_markdown_separator_line(line))
        .map(|line| {
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            match rest.strip_prefix('#') {
                Some(after) => format!("{indent}＃{after}"),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// タイトル末尾のサイト名を区切る記号。` - ` は前後の空白を必須にする（「Wi-Fi」「GPT-5」を切らないため）。
const TITLE_SITE_SEPARATORS: &[&str] = &["｜", "|", " - ", "："];
/// サイト名の前のカテゴリ（「… | 政治・経済・投資 | 東洋経済オンライン」）とみなす最大文字数。
const TITLE_CATEGORY_MAX_CHARS: usize = 20;
/// サイト名・カテゴリを外した後に残すタイトルの最小文字数。これより短くなるなら外さない。
const TITLE_MIN_REMAINING_CHARS: usize = 8;
/// 出典名の先頭の語のまとまりとして一致させるサイト名の最小文字数（「AI」のような短い語で前方一致させない）。
const TITLE_SITE_PREFIX_MIN_CHARS: usize = 4;

/// AI の種に使うタイトルから、末尾のサイト名（と、その前のカテゴリ）を外す。
///
/// RSS のタイトルが「記事タイトル | カテゴリ | サイト名」の形の媒体（東洋経済オンラインなど）では、
/// サイト名・カテゴリが要点・注目ポイント・タグに入っていた（実ニュース評価 v3）。外すのは AI の種だけで、
/// 保存するタイトル・画面の表示・重複判定のキー（出典名×タイトル）・記事 ID は変えない
/// （保存済みの記事と重複判定がずれて同じ記事を二重に保存したり、既存ファイルの移行が要ったりしないため）。
///
/// 誤って本文のタイトルを削らないよう、次のときだけ外す（どれかに外れればタイトルをそのまま返す）。
/// - 最後の区切り（TITLE_SITE_SEPARATORS）より後ろが、出典名（RSS のチャンネル名）と一致するか、
///   出典名の先頭の語のまとまり（「ITmedia NEWS 最新記事一覧」に対する「ITmedia NEWS」）である。
///   既知のサイト名の一覧は持たない（媒体ごとの分岐をしない）。
/// - 外した後に TITLE_MIN_REMAINING_CHARS 文字以上残る。
///
/// サイト名を外した区切りが `|` / `｜` で、その前にも同じ記号の区切りがあり、その後ろが TITLE_CATEGORY_MAX_CHARS
/// 文字以下で、残りがその 2 倍以上かつ最小文字数以上あれば、カテゴリとみなして一緒に外す
/// （「【連載】第3回｜AIの未来 | サイト名」のように前半が短いときは外さない）。
fn seed_title_without_site_suffix<'a>(title: &'a str, source_name: &str) -> &'a str {
    let title_trimmed = title.trim();
    let Some((head, separator, site)) = split_last_title_segment(title_trimmed) else {
        return title;
    };
    if !is_source_site_name(site, source_name, separator)
        || head.chars().count() < TITLE_MIN_REMAINING_CHARS
    {
        return title;
    }
    if matches!(separator, "|" | "｜") {
        if let Some((rest, category_separator, category)) = split_last_title_segment(head) {
            let category_chars = category.chars().count();
            let rest_chars = rest.chars().count();
            if category_separator == separator
                && category_chars <= TITLE_CATEGORY_MAX_CHARS
                && rest_chars >= TITLE_MIN_REMAINING_CHARS
                && rest_chars >= category_chars * 2
            {
                return rest;
            }
        }
    }
    head
}

/// タイトルを最後の区切りで分け、（前、区切り、後ろ）を返す。前後の空白は除く。後ろが空なら None。
fn split_last_title_segment(title: &str) -> Option<(&str, &'static str, &str)> {
    let (index, separator) = TITLE_SITE_SEPARATORS
        .iter()
        .filter_map(|separator| title.rfind(separator).map(|index| (index, *separator)))
        .max_by_key(|(index, _)| *index)?;
    let head = title[..index].trim_end();
    let tail = title[index + separator.len()..].trim();
    (!head.is_empty() && !tail.is_empty()).then_some((head, separator, tail))
}

/// 出典名の前方一致で、サイト名の後ろに続いてよい一般的な説明（チャンネル名の飾り）。大文字小文字は問わない。
/// 「Nintendo Switch」の「Switch」のような固有の語が続くときは、サイト名とみなさない。
const SOURCE_NAME_GENERIC_TAILS: &[&str] = &[
    "最新記事一覧",
    "最新記事",
    "記事一覧",
    "新着",
    "新着記事",
    "新着情報",
    "ニュース",
    "rss",
    "公式",
];

/// タイトル末尾の区切りの後ろ（segment）が出典名のサイト名か。空白の連なりは 1 つにそろえ、英字の大小は問わない。
/// - 出典名と一致する（2 文字以上）。
/// - または、出典名がその語で始まり（TITLE_SITE_PREFIX_MIN_CHARS 文字以上）、残りが区切り記号・括弧で始まるか
///   （「NHKニュース|社会」の「|社会」）、一般的な説明（SOURCE_NAME_GENERIC_TAILS。「ITmedia NEWS 最新記事一覧」の
///   「最新記事一覧」）だけのとき。区切りが `：` のときは前方一致を使わない（「新作を発表：Nintendo」のように
///   `：` の後ろは記事の主語・対象であることが多いため）。
fn is_source_site_name(segment: &str, source_name: &str, separator: &str) -> bool {
    let normalize = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let segment = normalize(segment);
    let source = normalize(source_name);
    if segment.chars().count() < 2 || source.is_empty() {
        return false;
    }
    if segment == source {
        return true;
    }
    if separator == "：" || segment.chars().count() < TITLE_SITE_PREFIX_MIN_CHARS {
        return false;
    }
    let Some(rest) = source.strip_prefix(&segment) else {
        return false;
    };
    let is_separator = |ch: char| {
        matches!(
            ch,
            '|' | '｜' | '-' | '―' | '—' | '–' | ':' | '：' | '(' | '（' | '['
        )
    };
    // 語の途中で切れていない（直後が空白か区切り記号・括弧）。
    let Some(first) = rest.chars().next() else {
        return false;
    };
    if is_separator(first) {
        return true;
    }
    if !first.is_whitespace() {
        return false;
    }
    // 空白の後ろが区切り記号（「Full-Count（フルカウント） ― 野球ニュース…」）か、一般的な説明だけ。
    let tail = rest.trim();
    tail.chars().next().is_some_and(is_separator) || SOURCE_NAME_GENERIC_TAILS.contains(&tail)
}

/// 種の組み立てに使う外部由来フィールドだけを無害化した記事のコピーを返す（保存には使わない）。
/// タイトルは末尾のサイト名・カテゴリを外してから無害化する（`seed_title_without_site_suffix`）。
fn neutralize_seed_article(article: &ArticleDetailDto) -> ArticleDetailDto {
    let mut seed_article = article.clone();
    seed_article.title = neutralize_seed_text(
        seed_title_without_site_suffix(&article.title, &article.source_name),
        SEED_TITLE_MAX_CHARS,
    );
    seed_article.genre = neutralize_seed_text(&article.genre, SEED_GENRE_MAX_CHARS);
    seed_article.excerpt = article
        .excerpt
        .as_deref()
        .map(|excerpt| neutralize_seed_text(excerpt, SEED_EXCERPT_MAX_CHARS))
        // 無害化で空になった抜粋は「抜粋なし」と同じ扱いにし、タイトルからの定型文へ切り替える。
        .filter(|excerpt| !excerpt.trim().is_empty());
    seed_article.focus_points = article
        .focus_points
        .iter()
        .map(|point| neutralize_seed_text(point, SEED_FOCUS_POINT_MAX_CHARS))
        .collect();
    seed_article
}

/// 保存メタ ai_provider の値。1つでも Mock に切り替わった出力があれば "mock"、
/// すべて同じ実AIなら そのプロバイダ名を返す（実AIで生成したように見せないため）。
fn combined_provider(responses: &[&AiResponse]) -> String {
    if responses
        .iter()
        .any(|response| response.provider == PROVIDER_MOCK)
    {
        return PROVIDER_MOCK.to_string();
    }
    responses
        .first()
        .map(|response| response.provider.clone())
        .unwrap_or_else(|| PROVIDER_MOCK.to_string())
}

/// Mock への切り替え結果（Gemini の通信失敗・キー未設定による代替）を保存してよいか。
/// 手動生成は保存し、自動要約は保存しない。実AIの出力の検証落ちはどちらも保存しない（D104）。
/// 自動要約（RejectFallback）は、要約済みの記事を上書きしない保存も兼ねる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackPolicy {
    SaveFallback,
    RejectFallback,
}

/// 自動要約1件の結果。キューはどちらも「完了」として扱い、失敗回数に数えない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoSummaryOutcome {
    /// 実AIの出力を保存した。
    Saved,
    /// AI 呼び出し中に手動要約などで要約済みになっていたため、上書きせずに保存をやめた。
    AlreadySummarized,
    /// 同じ記事の手動要約が始まったため、残りの段をやめた（保存しない）。手動要約が失敗した場合は、
    /// 次の投入（ニュース取得後・起動時・設定保存後）でまた並ぶ。
    YieldedToManual,
}

/// 要約生成本体の結果。`AlreadySummarized` は自動要約（RejectFallback）でだけ返る。
#[derive(Debug)]
enum SummaryGeneration {
    Generated(GeneratedArticleSummaryDto),
    AlreadySummarized,
}

/// 自動要約で、出力が Mock の代替に切り替わっていたら直ちにエラーを返す（残りの AI 呼び出しをしない）。
/// 手動生成（SaveFallback）では何もしない（Gemini の通信失敗などによる Mock の結果は従来どおり保存する）。
fn reject_fallback_early(
    fallback_policy: FallbackPolicy,
    article_id: &str,
    response: &AiResponse,
) -> Result<(), AppError> {
    if fallback_policy == FallbackPolicy::RejectFallback && response.provider == PROVIDER_MOCK {
        // 本文は出さず、記事IDだけを残す。
        log::warn!(
            "auto summary for {article_id} fell back to the mock provider; the remaining AI calls were skipped and nothing was saved"
        );
        return Err(AppError::Network(
            "real AI output was unavailable; fallback output was not saved".to_string(),
        ));
    }
    Ok(())
}

/// 検証対象の出力種別。ログには本文の代わりにこの種別名だけを出す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryOutputKind {
    Summary,
    Explanation,
    Comment,
    /// 要点・注目ポイント・タグ（JSON `{"key_points": [..], "focus_points": [..], "tags": [..]}`）。
    Points,
}

impl SummaryOutputKind {
    fn max_chars(self) -> usize {
        match self {
            Self::Summary => SUMMARY_MAX_CHARS,
            Self::Explanation => EXPLANATION_MAX_CHARS,
            Self::Comment => COMMENT_MAX_CHARS,
            // 要点・注目ポイントは1件あたりの上限（件数と応答全体の大きさは別に検証する）。
            Self::Points => POINT_ITEM_MAX_CHARS,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Explanation => "yuuko_explanation",
            Self::Comment => "yuuko_comment",
            Self::Points => "article_points",
        }
    }
}

/// 検証に落ちた理由。固定の分類だけを持ち、出力本文は保持しない（ログ・エラーへ本文を出さないため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRejection {
    Empty,
    TooLong,
    MarkdownHeading,
    MarkdownSeparator,
    HtmlTag,
    ControlCharacter,
    /// 要点・注目ポイント: 期待した JSON として解析できない。
    InvalidJson,
    /// 要点・注目ポイント: 件数が範囲外。
    ItemCount,
    /// 要点・注目ポイント: 1件の中に改行がある（記事ファイルの箇条書き1行に収まらない）。
    MultiLine,
    /// タグ: 出力に含まれていない（`tags` が無い・null）。
    Missing,
    /// タグ: カンマ・読点などの区切り文字を含む。
    Separator,
}

impl OutputRejection {
    fn label(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLong => "too_long",
            Self::MarkdownHeading => "markdown_heading",
            Self::MarkdownSeparator => "markdown_separator",
            Self::HtmlTag => "html_tag",
            Self::ControlCharacter => "control_character",
            Self::InvalidJson => "invalid_json",
            Self::ItemCount => "item_count",
            Self::MultiLine => "multi_line",
            Self::Missing => "missing",
            Self::Separator => "separator",
        }
    }
}

/// 要約・再説明・感想の出力を検証し、保存してよい trim 済み文字列を返す。
///
/// 見出し・区切り・HTML は「無害化して保存」ではなく「拒否して安全側へ切り替え」にする。
/// 理由: §12.5 は「含まないか」の確認を求めるだけで変換方法を定めておらず、用語解説側も
/// 不正出力は加工せず拒否している。また行頭「#」は記事Markdownのセクション見出し（`## AI要約` など）と、
/// 「---」は front matter 区切りと衝突しうるため、部分的に書き換えて残すより丸ごと捨てる方が確実に安全。
fn validate_summary_output(kind: SummaryOutputKind, text: &str) -> Result<String, OutputRejection> {
    if kind == SummaryOutputKind::Points {
        // 要点・注目ポイント・タグは JSON。要点・注目ポイントが検証を通れば、コードフェンスを除いた JSON を返す。
        // 検証済みの値へ組み直さずに返すのは、保存直前の `article_points_from_response` で同じ検証を
        // もう一度行い、タグを採用しなかった理由（固定ラベル）をそこで1回だけログに出すため。
        validate_article_points(text)?;
        return Ok(strip_enclosing_code_fence(text).to_string());
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(OutputRejection::Empty);
    }
    // バイト数ではなく Unicode 文字数で数える（用語解説側と同じ）。
    if trimmed.chars().count() > kind.max_chars() {
        return Err(OutputRejection::TooLong);
    }
    // 改行・タブ以外の制御文字は表示上危険なため拒否する（§12.5「表示上危険な文字列」）。
    if contains_disallowed_control_char(trimmed) {
        return Err(OutputRejection::ControlCharacter);
    }
    for line in trimmed.lines() {
        let line = line.trim();
        // 行頭「#」は Markdown 見出しとして記事のセクション構造（`## ...`）を壊しうる。
        if line.starts_with('#') {
            return Err(OutputRejection::MarkdownHeading);
        }
        // 「---」等の区切り行は front matter 区切りや水平線として解釈されうる。
        if is_markdown_separator_line(line) {
            return Err(OutputRejection::MarkdownSeparator);
        }
    }
    if contains_html_tag(trimmed) {
        return Err(OutputRejection::HtmlTag);
    }
    Ok(trimmed.to_string())
}

/// 要点・注目ポイントの応答全体の上限（バイト）。用語解説の JSON 応答と同じく、解析前に大きさで拒否する。
const ARTICLE_POINTS_RESPONSE_MAX_BYTES: usize = 16 * 1024;

/// 要点・注目ポイント・タグの AI の生の出力。
/// 要点・注目ポイントは厳格に解析する（未知のキーは拒否）。タグは型を問わず受け取り（無い・null も可）、
/// 別に検証する。タグの型崩れ（文字列・数値の配列など）で JSON 全体の解析が落ち、要点・要約まで
/// 保存されなくなるのを防ぐため（D11: タグの失敗は要約の保存を止めない）。
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArticlePoints {
    key_points: Vec<String>,
    focus_points: Vec<String>,
    #[serde(default)]
    tags: serde_json::Value,
}

/// 検証済みの要点・注目ポイント・タグと、タグを採用しなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedArticlePoints {
    /// `tags` は検証を通ったときだけ値が入り、通らなかったときは空。
    points: AiArticlePoints,
    /// タグを採用しなかった理由（採用したときは None）。要点・要約の保存には影響しない。
    tags_rejection: Option<OutputRejection>,
}

/// 要点・注目ポイント・タグの AI 出力を検証し、trim 済みの各項目を返す（副作用なし）。
///
/// - 解析前に応答全体の大きさ（16KiB）を確認し、厳格な JSON（未知のキーは拒否）として解析する。
///   Gemini は JSON だけを頼んでも ```json のコードフェンスで囲むことがあるため、全体を囲む1組だけは外す。
/// - 件数（要点 2〜4・注目ポイント 1〜3）と、各項目の文字数（100文字）・文字種を確認する。
/// - 各項目は記事ファイルで `- 項目` の1行として保存するため、改行・行頭「#」・区切り行・HTML・制御文字は
///   無害化せず拒否する（要約などと同じく、部分的に書き換えて残すより丸ごと捨てる方が確実に安全）。
/// - タグ（3〜5件）は `validate_article_tags` で別に検証する。タグが不正なら要点・注目ポイントは
///   そのまま返し、タグだけを空にする（D11）。
fn validate_article_points(text: &str) -> Result<ValidatedArticlePoints, OutputRejection> {
    if text.len() > ARTICLE_POINTS_RESPONSE_MAX_BYTES {
        return Err(OutputRejection::TooLong);
    }
    let parsed = serde_json::from_str::<RawArticlePoints>(strip_enclosing_code_fence(text))
        .map_err(|_| OutputRejection::InvalidJson)?;
    let key_points = validate_point_items(
        &parsed.key_points,
        KEY_POINTS_MIN_ITEMS..=KEY_POINTS_MAX_ITEMS,
    )?;
    let focus_points = validate_point_items(
        &parsed.focus_points,
        FOCUS_POINTS_MIN_ITEMS..=FOCUS_POINTS_MAX_ITEMS,
    )?;
    let (tags, tags_rejection) = match validate_article_tags(&parsed.tags) {
        Ok(tags) => (tags, None),
        Err(rejection) => (Vec::new(), Some(rejection)),
    };
    Ok(ValidatedArticlePoints {
        points: AiArticlePoints {
            key_points,
            focus_points,
            tags,
        },
        tags_rejection,
    })
}

/// 記事タグの AI 出力を検証し、trim・重複除去済みのタグ（3〜5件）を返す（副作用なし）。
///
/// - 文字列の配列であること。1つでも不正な要素があれば、部分的に残さずタグ全体を採用しない
///   （要点・注目ポイントと同じく、書き換えて残すより丸ごと捨てる方が確実に安全）。
/// - 重複は大文字・小文字を区別せずに除き（先に出た方を残す）、除いた後の件数で 3〜5件を確かめる。
fn validate_article_tags(value: &serde_json::Value) -> Result<Vec<String>, OutputRejection> {
    let items = match value {
        serde_json::Value::Null => return Err(OutputRejection::Missing),
        serde_json::Value::Array(items) => items,
        _ => return Err(OutputRejection::InvalidJson),
    };
    // 重複除去の前に、生の件数を上限の2倍で切る（極端に長い配列を1件ずつ検証しない）。
    // 3〜5件の確認は重複を除いた後に行う（["AI", "ai", ...] のような重複で落とさないため）。
    if items.len() > ARTICLE_TAGS_MAX_ITEMS * 2 {
        return Err(OutputRejection::ItemCount);
    }
    let mut tags: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        let tag = validate_article_tag(item.as_str().ok_or(OutputRejection::InvalidJson)?)?;
        let folded = tag.to_lowercase();
        if !tags
            .iter()
            .any(|existing| existing.to_lowercase() == folded)
        {
            tags.push(tag);
        }
    }
    if !(ARTICLE_TAGS_MIN_ITEMS..=ARTICLE_TAGS_MAX_ITEMS).contains(&tags.len()) {
        return Err(OutputRejection::ItemCount);
    }
    Ok(tags)
}

/// タグ1件を検証し、trim 済みの値を返す。
/// タグは記事ファイルの front matter（YAML の配列 `tags`）へ保存する。YAML としては serde_yaml が
/// 必要に応じて引用符を付けるため往復で壊れないが、画面・おすすめ判定でタグとして扱いやすいよう、
/// 1行・短い・区切り文字を含まない値だけを受け付ける（先頭の `#`・`＃` は取り除く）。
fn validate_article_tag(item: &str) -> Result<String, OutputRejection> {
    // 小さいモデル（ローカルLLM）は指示しても `#タグ` のハッシュタグ形式で返しがちで、拒否するとタグが
    // 全部捨てられる。先頭の `#`・`＃` は意味を持たない飾りなので、拒否せず取り除いてから検証する。
    // `# #AI` のように記号と空白が交互に続いても残らないよう、両方をまとめて取り除く。
    let trimmed = item
        .trim()
        .trim_start_matches(|c: char| c == '#' || c == '＃' || c.is_whitespace());
    if trimmed.is_empty() {
        return Err(OutputRejection::Empty);
    }
    if trimmed.chars().count() > ARTICLE_TAG_MAX_CHARS {
        return Err(OutputRejection::TooLong);
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(OutputRejection::MultiLine);
    }
    // タブも含めて制御文字は拒否する（タグは1語の短い値なので、要約のように改行・タブを許す理由がない）。
    if trimmed.contains('\t') || contains_disallowed_control_char(trimmed) {
        return Err(OutputRejection::ControlCharacter);
    }
    // HTML の山括弧は、タグの形になっていなくても拒否する（短い値なので比較の `<` を許す必要がない）。
    if trimmed.contains(['<', '>']) {
        return Err(OutputRejection::HtmlTag);
    }
    if trimmed.contains(ARTICLE_TAG_FORBIDDEN_CHARS) {
        return Err(OutputRejection::Separator);
    }
    Ok(trimmed.to_string())
}

/// 応答全体が1組のコードフェンス（```json … ``` / ``` … ```）で囲まれていれば中身を返す。
/// それ以外（前後に文章がある等）はそのまま返し、JSON 解析で落とす。
fn strip_enclosing_code_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let Some(body) = rest.strip_suffix("```") else {
        return trimmed;
    };
    // 開きフェンスの行（```json など）を飛ばす。
    match body.split_once('\n') {
        Some((info, inner)) if info.trim().chars().all(|c| c.is_ascii_alphanumeric()) => {
            inner.trim()
        }
        _ => trimmed,
    }
}

fn validate_point_items(
    items: &[String],
    allowed_count: std::ops::RangeInclusive<usize>,
) -> Result<Vec<String>, OutputRejection> {
    if !allowed_count.contains(&items.len()) {
        return Err(OutputRejection::ItemCount);
    }
    items.iter().map(|item| validate_point_item(item)).collect()
}

fn validate_point_item(item: &str) -> Result<String, OutputRejection> {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return Err(OutputRejection::Empty);
    }
    if trimmed.chars().count() > POINT_ITEM_MAX_CHARS {
        return Err(OutputRejection::TooLong);
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(OutputRejection::MultiLine);
    }
    if contains_disallowed_control_char(trimmed) {
        return Err(OutputRejection::ControlCharacter);
    }
    if trimmed.starts_with('#') {
        return Err(OutputRejection::MarkdownHeading);
    }
    // 保存形（`- 項目`）でも区切り行にならないことを確かめる（例: 項目 `--` → `- --`）。
    if is_markdown_separator_line(trimmed) || is_markdown_separator_line(&format!("- {trimmed}")) {
        return Err(OutputRejection::MarkdownSeparator);
    }
    if contains_html_tag(trimmed) {
        return Err(OutputRejection::HtmlTag);
    }
    Ok(trimmed.to_string())
}

/// 「-」が3つ以上で、他は空白だけの行（`---` / `- - -` など）を区切り行とみなす。
fn is_markdown_separator_line(line: &str) -> bool {
    let mut dash_count = 0;
    for c in line.chars() {
        match c {
            '-' => dash_count += 1,
            c if c.is_whitespace() => {}
            _ => return false,
        }
    }
    dash_count >= 3
}

/// AI 出力を検証し、通った出力だけを返す。ログには出力種別と拒否理由の固定ラベルだけを出し、
/// 出力本文・記事本文は出さない。
///
/// 実AI（Gemini・ローカル）の出力が検証に落ちたときは、手動の要約生成でも Mock の結果へ
/// 切り替えずに `AppError::AiOutputRejected` を返す（判断台帳 D104。記事は未要約のまま・
/// 保存済みの要約は上書きしない。自動要約の D56 と同じ扱い）。Mock の定型文を実AIの要約のように
/// 保存すると、利用者が作り直す理由に気づけないため。
/// Mock を選んでいる利用者（provider = "mock"）の出力は、従来どおり検証に通れば使う。
/// 落ちた場合は `AppError::Parse`（Mock の種は無害化済みのため通常は起きない）。
/// なお、Gemini の通信失敗・キー未設定による Mock への切り替え（`AiProviderService`）は従来どおり。
///
/// 実AIの再説明・一言は、検証の前にト書き・中国語の助詞・挨拶を取り除く（`clean_ai_speech`）。
/// 重いニュース（`SpeechTone::Calm`・D122）では、絵文字・「！」なども取り除く。
/// `input_text` はその出力を作った AI 入力（中国語の字・英単語の混入を、入力にあるものと区別するため）。
fn select_valid_output_in_tone(
    kind: SummaryOutputKind,
    tone: SpeechTone,
    input_text: &str,
    primary: AiResponse,
) -> Result<AiResponse, AppError> {
    let primary = strip_ai_headings(kind, primary);
    let primary = clean_ai_speech(kind, tone, input_text, primary);
    let rejection = match validate_summary_output(kind, &primary.text) {
        Ok(text) => {
            return Ok(AiResponse {
                text,
                provider: primary.provider,
            })
        }
        Err(rejection) => rejection,
    };

    log::warn!(
        "AI summary output was rejected ({}: {}); the generated summary was not saved",
        kind.label(),
        rejection.label()
    );
    Err(rejection_error(&primary))
}

/// 実AIの要約・再説明・一言から、消しても文が壊れない見出し・ラベル（「**中心となる出来事の要約:**」
/// 「ゆうこの一言：」・見出しだけの行・太字の記号）を消す（D170。`consistency_check::strip_heading_labels`）。
/// 消すだけなので、続く保存前の検証（見出し・HTML・制御文字・文字数）はそのまま効く。Mock の結果は触らない。
fn strip_ai_headings(kind: SummaryOutputKind, response: AiResponse) -> AiResponse {
    if kind == SummaryOutputKind::Points || response.provider == PROVIDER_MOCK {
        return response;
    }
    let (text, removed) = strip_heading_labels(&response.text);
    if removed == 0 {
        return response;
    }
    // 本文は出さず、数と出力種別だけを残す。
    log::info!(
        "removed {removed} heading(s) or label(s) from the AI output ({})",
        kind.label()
    );
    AiResponse {
        text,
        provider: response.provider,
    }
}

/// 実AIの要約・再説明・一言の2文目以降の文頭のつなぎの語（「これにより、」「その結果、」など）と直後の読点を消す
/// （Firestore タスク I9iRpb。`leading_connector::strip_leading_connectors`）。記事にない因果の言い足しを減らすため。
/// `source` は点検の材料（タイトル＋本文抜粋）で、記事自体が同じ因果を同じ語で書いているときは消さない。
/// 消すだけなので、先に通った保存前の検証（§12.5）の結果は変わらない（1文目は消さないので空にもならない）。
/// Mock の結果・要点の段は触らない（`strip_ai_headings` と同じ）。
fn strip_ai_leading_connectors(
    kind: SummaryOutputKind,
    response: AiResponse,
    source: &str,
) -> AiResponse {
    if kind == SummaryOutputKind::Points || response.provider == PROVIDER_MOCK {
        return response;
    }
    let (text, removed) = strip_leading_connectors(source, &response.text);
    if removed == 0 {
        return response;
    }
    // 本文は出さず、数と出力種別だけを残す。
    log::info!(
        "removed {removed} leading connector(s) from the AI output ({})",
        kind.label()
    );
    AiResponse {
        text,
        provider: response.provider,
    }
}

/// 実AI（Gemini・ローカル）の再説明・一言から、話の中身ではない飾り（括弧のト書き・中国語の助詞・
/// 冒頭の挨拶・締めの挨拶）を取り除く。拒否すると記事が未要約のまま残る（D104 / D56）ため、捨てずに削る。
/// 除去は文字を消すだけなので、続く `validate_summary_output` の検証（HTML・制御文字・見出し・文字数）は
/// そのまま効く。すべて消えたときは空になり、検証の Empty で拒否される。
/// Mock の種（定型文）は手を加えず、決定的な結果のままにする。要約・要点は中立な文体なので対象外。
/// 重いニュース（`SpeechTone::Calm`・D122）では、続けて `calm_yuuko_speech` で明るさの飾り
/// （「♪」「★」を含む絵文字・記号、「！」、語尾の「～」）を取り除く。
/// 日本語に無い簡体字を含む文は、ほかの文が残るときだけ取り除く（`strip_foreign_script_sentences`）。
/// 入力に無い英単語は、固有名詞の表記ゆれと区別できないため消さず、数だけをログに残す。
/// 文末の丁寧語（「〜しました。」「〜です。」）は、活用の形が決まるものだけゆうこの常体の語尾
/// （「〜したよ。」「〜だよ。」）へ言い換える（`casualize_polite_endings`）。
/// 失礼な呼びかけ（「お前ら」「てめえ」）は、文頭の呼びかけなら落とし、ほかは「みんな」へ言い換える
/// （`soften_rude_address`）。同じ文の繰り返しは最初の1つだけを残す（`strip_repeated_sentences`）。
fn clean_ai_speech(
    kind: SummaryOutputKind,
    tone: SpeechTone,
    input_text: &str,
    response: AiResponse,
) -> AiResponse {
    if !matches!(
        kind,
        SummaryOutputKind::Explanation | SummaryOutputKind::Comment
    ) || response.provider == PROVIDER_MOCK
    {
        return response;
    }
    let cleaned = clean_yuuko_speech(&response.text);
    if cleaned != response.text.trim() {
        // 本文は出さず、取り除いたことと出力種別だけを残す。
        log::info!(
            "removed stage directions, greetings or non-Japanese particles from the AI output ({})",
            kind.label()
        );
    }
    // 失礼な呼びかけ（「ねえ、お前ら、」「てめえ」）を落とす・「みんな」へ言い換える（引用の中は触らない）。
    let (cleaned, rude_terms) = soften_rude_address(&cleaned);
    if rude_terms > 0 {
        // 本文は出さず、数と出力種別だけを残す。
        log::info!(
            "removed or replaced {rude_terms} rude form(s) of address in the AI output ({})",
            kind.label()
        );
    }
    // プロンプトの固定指示の写し（「ゆうこは読者の横で…案内役です。」）の文を取り除く。
    let (cleaned, prompt_echoes) =
        strip_prompt_echo_sentences(&cleaned, &yuuko_speech_fixed_prompt_texts());
    if prompt_echoes > 0 {
        // 本文は出さず、数と出力種別だけを残す。
        log::info!(
            "removed {prompt_echoes} sentence(s) copying the fixed prompt from the AI output ({})",
            kind.label()
        );
    }
    // 文末の丁寧語（です・ます調）を、活用の決まる形だけゆうこの常体の語尾へ言い換える（入力の要約の文体の写し対策）。
    let (cleaned, casualized) = casualize_polite_endings(&cleaned);
    if casualized > 0 {
        // 本文は出さず、数と出力種別だけを残す。
        log::info!(
            "rewrote {casualized} polite sentence ending(s) in the AI output ({})",
            kind.label()
        );
    }
    // 同じ文の繰り返し（語尾の言い換えでそろった文を含む）は、最初の1つだけを残す。
    let (cleaned, repeated) = strip_repeated_sentences(&cleaned);
    if repeated > 0 {
        // 本文は出さず、数と出力種別だけを残す。
        log::info!(
            "removed {repeated} repeated sentence(s) from the AI output ({})",
            kind.label()
        );
    }
    let (cleaned, foreign_script) = strip_foreign_script_sentences(&cleaned, input_text);
    if foreign_script != ForeignScriptOutcome::Clean {
        // 本文・混入した字は出さず、固定ラベルだけを残す。
        log::info!(
            "found Chinese-only characters in the AI output ({}: {})",
            kind.label(),
            foreign_script.label()
        );
    }
    let unlisted_latin_words = count_unlisted_latin_words(&cleaned, input_text);
    if unlisted_latin_words > 0 {
        // 単語そのものは出さず、数だけを残す（消さない: 固有名詞の表記ゆれと区別できないため）。
        log::info!(
            "the AI output has {unlisted_latin_words} Latin word(s) not found in its input ({})",
            kind.label()
        );
    }
    let cleaned = match tone {
        SpeechTone::Normal => cleaned,
        SpeechTone::Calm => {
            let calmed = calm_yuuko_speech(&cleaned);
            if calmed != cleaned {
                log::info!(
                    "removed emoji or exclamations from the AI output for heavy news ({})",
                    kind.label()
                );
            }
            calmed
        }
    };
    AiResponse {
        text: cleaned,
        provider: response.provider,
    }
}

/// 検証に落ちた出力のエラー。Mock の出力なら `Parse`、実AIの出力なら `AiOutputRejected`（D104）。
fn rejection_error(response: &AiResponse) -> AppError {
    if response.provider == PROVIDER_MOCK {
        return AppError::Parse("ai summary output failed validation".to_string());
    }
    AppError::AiOutputRejected
}

/// 要点・注目ポイント・タグの応答を、保存できる形（検証済みの各項目）へ変換する。
/// 注入された取得手段が検証を通していなくても、形の崩れた出力を保存しないよう、ここで必ず検証する。
/// タグだけが検証に落ちた場合はエラーにせず、タグを空にして返す（呼び出し側は既存のタグを残す・D11）。
fn article_points_from_response(response: &AiResponse) -> Result<AiArticlePoints, AppError> {
    let validated = validate_article_points(&response.text).map_err(|rejection| {
        log::warn!(
            "AI summary output was rejected ({}: {}); the generated summary was not saved",
            SummaryOutputKind::Points.label(),
            rejection.label()
        );
        rejection_error(response)
    })?;
    if let Some(rejection) = validated.tags_rejection {
        // 本文・タグの値は出さず、固定ラベルだけを残す。
        log::warn!(
            "AI article tags were rejected ({}); the summary is saved and the existing tags are kept",
            rejection.label()
        );
    }
    Ok(validated.points)
}

fn build_focus_points(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
) -> Vec<String> {
    let title = &article.title;
    let genre = &article.genre;
    let source_points = if article.focus_points.is_empty() {
        vec![
            format!("{title} の要点を確認する"),
            format!("{genre} 分野での意味を捉える"),
            "元記事の背景と影響範囲を整理する".to_string(),
        ]
    } else {
        article.focus_points.clone()
    };

    match explanation_level {
        ExplanationLevel::Simple => source_points.into_iter().take(2).collect(),
        ExplanationLevel::Normal | ExplanationLevel::Detailed => source_points,
    }
}

/// 再説明・一言の実AI入力。無害化済みタイトル・同じ生成の要約・要点を、ラベル付きの短い行で並べる。
///
/// 要約・要点は §12.5 の検証を通った AI 出力だが、記事（外部データ）から作られたものなので信頼せず、
/// 種と同じ `neutralize_seed_text` で行頭 `#`・`<`・区切り行・制御文字を無害化し、長さを切り詰める。
/// ローカルLLMでは入力の長さがそのままプロンプト処理時間になるため、本文抜粋（最大1500文字）ではなく
/// 短い要約・要点を渡す。実AIの出力なら通常は合計で数百文字で、上限はタイトル200＋要約400＋要点4件×100文字。
/// 注目ポイントは要点の言い換えになりやすいので入れない。
/// `summary` が `None`（要約が Mock の代替＝本文の短い抜粋だったとき）は要約の行を入れず、
/// タイトルと要点だけにする（定型文を実AIへの入力に混ぜないため）。
pub(crate) fn build_yuuko_speech_input(
    title: &str,
    summary: Option<&str>,
    key_points: &[String],
) -> String {
    let mut lines = vec![format!(
        "タイトル: {}",
        neutralize_seed_text(title, SEED_TITLE_MAX_CHARS)
    )];
    if let Some(summary) = summary {
        lines.push(format!(
            "要約: {}",
            neutralize_seed_text(summary.trim(), SEED_SUMMARY_MAX_CHARS)
        ));
    }
    lines.push("要点:".to_string());
    lines.extend(key_points.iter().take(KEY_POINTS_MAX_ITEMS).map(|point| {
        format!(
            "・{}",
            neutralize_seed_text(point.trim(), POINT_ITEM_MAX_CHARS)
        )
    }));
    lines.join("\n")
}

/// 要約（`summary_v2`・D167）の実AI入力。無害化済みのタイトル・本文抜粋と、同じ生成で先に作った要点を、
/// ラベル付きの行で並べる（`タイトル: …` / `本文抜粋:` に続く抜粋 / `要点:` に続く `・…` の行）。
///
/// 要点は §12.5 の検証を通った AI 出力だが、記事（外部データ）から作られたものなので信頼せず、
/// 再説明・一言の入力と同じく `neutralize_seed_text` で無害化し、各100文字・最大4件に切り詰める。
/// タイトル・本文抜粋は呼び出し側で無害化済み（`neutralize_seed_article`。タイトル200・抜粋1500文字）だが、
/// 単体で使っても安全なように、ここでも同じ上限で無害化する（無害化済みの文は変わらない）。
/// 要点を本文抜粋の後ろ（生成の直前）に置くのは、小さいモデルは入力の末尾に近い材料ほど使いやすいため。
/// `key_points` が `None`・空（要点が Mock の代替だったとき・作り直し）は要点の行を入れない。
/// 抜粋が無い記事はタイトル（と要点）だけにする。
/// 外部由来の文の行頭にある見出し（`タイトル:` / `本文抜粋:` / `要点:`）はコロンを全角にする
/// （`neutralize_summary_input_labels`）。本文抜粋に「要点:」の行があると偽の要点の区切りになり、
/// 指示の選び分け（入力に要点があるか）もだまされるため。
pub(crate) fn build_summary_input(
    title: &str,
    excerpt: Option<&str>,
    key_points: Option<&[String]>,
) -> String {
    let external = |text: &str, max_chars: usize| {
        neutralize_summary_input_labels(&neutralize_seed_text(text, max_chars))
    };
    let mut lines = vec![format!(
        "{SUMMARY_INPUT_TITLE_LABEL} {}",
        external(title, SEED_TITLE_MAX_CHARS)
    )];
    if let Some(excerpt) = excerpt.filter(|excerpt| !excerpt.trim().is_empty()) {
        lines.push(SUMMARY_INPUT_EXCERPT_LABEL.to_string());
        lines.push(external(excerpt.trim(), SEED_EXCERPT_MAX_CHARS));
    }
    if let Some(key_points) = key_points.filter(|points| !points.is_empty()) {
        lines.push(SUMMARY_INPUT_KEY_POINTS_LABEL.to_string());
        lines.extend(
            key_points
                .iter()
                .take(KEY_POINTS_MAX_ITEMS)
                .map(|point| format!("・{}", external(point.trim(), POINT_ITEM_MAX_CHARS))),
        );
    }
    lines.join("\n")
}

/// 外部由来の文の各行で、行頭（先頭の空白の後）にある要約の入力の見出しのコロンを全角にする
/// （「要点:」→「要点：」）。文字数は変わらない。
fn neutralize_summary_input_labels(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            [
                SUMMARY_INPUT_TITLE_LABEL,
                SUMMARY_INPUT_EXCERPT_LABEL,
                SUMMARY_INPUT_KEY_POINTS_LABEL,
            ]
            .iter()
            .find_map(|label| {
                rest.strip_prefix(label).map(|after| {
                    let name = label.trim_end_matches(':');
                    format!("{indent}{name}：{after}")
                })
            })
            .unwrap_or_else(|| line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 要点・注目ポイントの種: 1行目に無害化済みタイトル、2行目以降に無害化済みの本文抜粋（無ければタイトルだけ）。
/// 実AIへの入力であり、Mock はこの種の文から要点を組む（ai_provider_service の mock_article_points）。
fn build_points_seed(article: &ArticleDetailDto) -> String {
    match article.excerpt.as_deref() {
        Some(excerpt) => format!("{}\n{excerpt}", article.title),
        None => article.title.clone(),
    }
}

/// 要約の Mock の結果の種（本文抜粋＋定型文）。Mock はこの先頭の短い抜粋を要約にする（`mock_excerpt`）。
/// D167 の前は実AIへの要約の入力でもあったが、いまは Mock 専用（`AiRequest.context` で渡し、実AIへは送らない）。
/// 実AIへの要約の入力は `build_summary_input`。
fn build_summary_seed(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
    focus_points: &[String],
) -> String {
    let title = &article.title;
    let fallback_genre = article.genre.clone();
    let primary_focus_point = focus_points
        .first()
        .cloned()
        .unwrap_or_else(|| fallback_genre.clone());
    let secondary_focus_point = focus_points
        .get(1)
        .cloned()
        .unwrap_or_else(|| "背景の変化".to_string());
    // 再生成時に AI要約を種にして膨張しないよう、種の基は元の本文抜粋(excerpt)とする。
    // excerpt が無い記事はタイトルベースの控えめな文にフォールバックする。
    let base_summary = article
        .excerpt
        .clone()
        .unwrap_or_else(|| format!("{title} に関する記事です。"));

    match explanation_level {
        ExplanationLevel::Simple => format!(
            "{base_summary}。まずは「{primary_focus_point}」を押さえると流れを掴みやすいです。"
        ),
        ExplanationLevel::Normal => {
            format!("{base_summary} 特に、{primary_focus_point}。")
        }
        ExplanationLevel::Detailed => format!(
            "{base_summary} この記事では、{primary_focus_point}。さらに、{secondary_focus_point} という観点まで追うと理解しやすいです。"
        ),
    }
}

/// 再説明の Mock 定型文。重いニュース（`SpeechTone::Calm`・D122）では、明るい言い回し
/// （「読みやすいよ」「現場や利用者に影響」など）を避けた落ち着いた文にする。ふだんの文は従来のまま。
fn build_yuuko_explanation_seed(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
    focus_points: &[String],
    tone: SpeechTone,
) -> String {
    let genre = &article.genre;
    let first_point = focus_points
        .first()
        .cloned()
        .unwrap_or_else(|| article.genre.clone());
    let secondary_focus_point = focus_points
        .get(1)
        .cloned()
        .unwrap_or_else(|| "関連する背景".to_string());

    if tone == SpeechTone::Calm {
        return match explanation_level {
            ExplanationLevel::Simple => format!(
                "この記事は、まず「{first_point}」を確かめると流れがつかめるよ。分かっている事実から、順番に落ち着いて読んでいこうね。"
            ),
            ExplanationLevel::Normal => format!(
                "この記事は、{genre} の出来事を伝えているよ。特に {first_point} について、分かっている事実と、まだはっきりしないことを分けて読むと、落ち着いて整理できるね。"
            ),
            ExplanationLevel::Detailed => format!(
                "この記事は、{genre} の出来事を扱っているよ。まずは {first_point} を押さえて、そのうえで {secondary_focus_point} を確かめてみてね。分かっている事実と、まだ確かでないことを分けて読むと、落ち着いて整理できるよ。"
            ),
        };
    }

    match explanation_level {
        ExplanationLevel::Simple => {
            format!("この記事は、まず「{first_point}」を見ると読みやすいよ。難しい用語より、何が変わるのかに注目するとつかみやすいはずだよ。")
        }
        ExplanationLevel::Normal => format!(
            "この記事は、{genre} を起点に読むと理解しやすいよ。特に {first_point} がどう現場や利用者に影響するかを見ると、話の流れが追いやすくなるね。"
        ),
        ExplanationLevel::Detailed => format!(
            "この記事は、{genre} の話題を扱っているよ。まずは {first_point} を押さえて、そのうえで {secondary_focus_point} がどう広がるかを見てみてね。技術面と実用面の両方が整理しやすくなるよ。"
        ),
    }
}

/// 一言の Mock 定型文。重いニュース（`SpeechTone::Calm`・D122）では「面白そう」「便利」のような
/// 明るい感想を避け、落ち着いた一言にする。ふだんの文は従来のまま。
fn build_yuuko_comment_seed(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
    tone: SpeechTone,
) -> String {
    let genre = &article.genre;
    let title = &article.title;
    if tone == SpeechTone::Calm {
        return match explanation_level {
            ExplanationLevel::Simple => {
                format!("{genre}の大事な出来事だから、確かな情報を落ち着いて確かめておきたいね。")
            }
            ExplanationLevel::Normal => {
                format!("{title}の話は、まず分かっている事実を落ち着いて受け止めたいね。")
            }
            ExplanationLevel::Detailed => {
                format!("{title}については、続報も含めて確かな情報を落ち着いて追っていきたいね。")
            }
        };
    }
    match explanation_level {
        ExplanationLevel::Simple => {
            format!("{genre}って、結局どこが便利になるのかを見ると分かりやすそうだね。")
        }
        ExplanationLevel::Normal => format!(
            "{title}の話だけど、仕組みより『使った先で何が変わるか』に目を向けると面白そうだね。"
        ),
        ExplanationLevel::Detailed => format!(
            "{title}の話題は専門的に見えても、実際には現場でどう役立つかまでつながると理解しやすいね。"
        ),
    }
}

/// 要約生成時刻（UTC・"YYYY-MM-DDThh:mm:ssZ"）。保存メタ summary_generated_at に使う。
/// news_service の取得時刻と同じ形式に揃える。
fn current_utc_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        build_focus_points, build_summary_seed, build_yuuko_comment_seed,
        build_yuuko_explanation_seed, build_yuuko_speech_input, combined_provider,
        neutralize_seed_article, neutralize_seed_text, seed_title_without_site_suffix,
        select_valid_output_in_tone, strip_ai_leading_connectors, validate_article_points,
        validate_point_item, validate_summary_output, FallbackPolicy, OutputRejection,
        SummaryGeneration, SummaryOutputKind, SummaryService, COMMENT_MAX_CHARS,
        EXPLANATION_MAX_CHARS, SEED_SUMMARY_MAX_CHARS, SUMMARY_MAX_CHARS,
    };
    use crate::domain::article::{ArticleReadState, ArticleSummaryUpdate, FetchedArticle};
    use crate::domain::settings::ExplanationLevel;
    use crate::domain::speech_tone::SpeechTone;
    use crate::domain::summary::{
        AiResponse, GenerateArticleSummaryParams, GeneratedArticleSummaryDto,
        ARTICLE_POINTS_PROMPT_ID, ARTICLE_TAG_MAX_CHARS, KEY_POINTS_MAX_ITEMS,
        KEY_POINTS_MIN_ITEMS, POINT_ITEM_MAX_CHARS, SUMMARY_PROMPT_ID,
    };
    use crate::error::{AppError, CommandError};
    use crate::paths::AppPaths;
    use crate::repositories::article_repository::ArticleRepository;
    use crate::repositories::settings_repository::SettingsRepository;
    use crate::services::ai_provider_service::AiProviderService;
    use std::cell::RefCell;

    const ARTICLE_ID: &str = "rss-20261007-tech-01";

    /// ふだんの口調で出力を選ぶ（重いニュースの判定を通さない既存のテスト用）。
    fn select_valid_output(
        kind: SummaryOutputKind,
        primary: AiResponse,
    ) -> Result<AiResponse, AppError> {
        select_valid_output_in_tone(kind, SpeechTone::Normal, "", primary)
    }

    fn response(text: &str, provider: &str) -> AiResponse {
        AiResponse {
            text: text.to_string(),
            provider: provider.to_string(),
        }
    }

    /// 出力種別に合った形の応答を作る。要点・注目ポイントは text を元にした有効な JSON にする。
    fn output(kind: SummaryOutputKind, text: &str, provider: &str) -> AiResponse {
        if kind == SummaryOutputKind::Points {
            return response(&points_json(text), provider);
        }
        response(text, provider)
    }

    fn points_json(text: &str) -> String {
        serde_json::json!({
            "key_points": [format!("{text}（要点1）"), format!("{text}（要点2）")],
            "focus_points": [format!("{text}（注目）")],
        })
        .to_string()
    }

    // --- validate_summary_output ---

    #[test]
    fn valid_output_is_accepted_and_trimmed() {
        let text = "  新しい半導体工場の計画が発表されました。\n地域の雇用にも影響しそうです。  ";
        let validated = validate_summary_output(SummaryOutputKind::Summary, text).unwrap();
        assert_eq!(
            validated,
            "新しい半導体工場の計画が発表されました。\n地域の雇用にも影響しそうです。"
        );
        // 比較の「<」やハイフン始まりの箇条書き風の行は拒否しない。
        assert!(validate_summary_output(SummaryOutputKind::Comment, "1 < 2 だね").is_ok());
        assert!(validate_summary_output(SummaryOutputKind::Comment, "- ポイントです").is_ok());
    }

    #[test]
    fn empty_or_whitespace_output_is_rejected() {
        for text in ["", "   ", "\n\t \n"] {
            assert_eq!(
                validate_summary_output(SummaryOutputKind::Summary, text),
                Err(OutputRejection::Empty)
            );
        }
    }

    #[test]
    fn heading_mixed_output_is_rejected() {
        for text in [
            "# 見出し",
            "要約です。\n## ゆうこの一言\n乗っ取り",
            "要約です。\n   ### 小見出し",
        ] {
            assert_eq!(
                validate_summary_output(SummaryOutputKind::Explanation, text),
                Err(OutputRejection::MarkdownHeading)
            );
        }
    }

    #[test]
    fn separator_mixed_output_is_rejected() {
        for text in ["前半\n---\n後半", "前半\n- - -\n後半", "前半\n-----"] {
            assert_eq!(
                validate_summary_output(SummaryOutputKind::Summary, text),
                Err(OutputRejection::MarkdownSeparator)
            );
        }
    }

    #[test]
    fn html_mixed_output_is_rejected() {
        for text in [
            "<script>alert(1)</script>",
            "要約は<b>重要</b>です。",
            "末尾に閉じタグ</p>",
            "コメント<!-- x -->",
        ] {
            assert_eq!(
                validate_summary_output(SummaryOutputKind::Comment, text),
                Err(OutputRejection::HtmlTag)
            );
        }
    }

    #[test]
    fn control_character_output_is_rejected() {
        assert_eq!(
            validate_summary_output(SummaryOutputKind::Comment, "一言\u{0007}です"),
            Err(OutputRejection::ControlCharacter)
        );
    }

    #[test]
    fn each_kind_rejects_output_over_its_char_limit() {
        // 上限ちょうどは許可し、1文字超過で拒否する（バイト数ではなく文字数で数える）。
        for (kind, limit) in [
            (SummaryOutputKind::Summary, SUMMARY_MAX_CHARS),
            (SummaryOutputKind::Explanation, EXPLANATION_MAX_CHARS),
            (SummaryOutputKind::Comment, COMMENT_MAX_CHARS),
        ] {
            assert!(validate_summary_output(kind, &"あ".repeat(limit)).is_ok());
            assert_eq!(
                validate_summary_output(kind, &"あ".repeat(limit + 1)),
                Err(OutputRejection::TooLong)
            );
        }
    }

    // --- select_valid_output（実AIの検証落ちは Mock へ切り替えない・D104） ---

    #[test]
    fn valid_real_ai_output_is_used_as_is() {
        for provider in ["gemini", "local"] {
            let selected = select_valid_output(
                SummaryOutputKind::Summary,
                response(" 正常な要約です。 ", provider),
            )
            .unwrap();
            assert_eq!(selected.text, "正常な要約です。");
            assert_eq!(selected.provider, provider);
        }
    }

    #[test]
    fn real_ai_explanation_and_comment_are_cleaned_before_validation() {
        let noisy = "（ゆうこが微笑んで手を振って）こんにちは、お元気ですか？哦～、新しい補助金が始まるよ。";
        for provider in ["gemini", "local"] {
            for kind in [SummaryOutputKind::Explanation, SummaryOutputKind::Comment] {
                let selected = select_valid_output(kind, response(noisy, provider)).unwrap();
                assert_eq!(
                    selected.text, "新しい補助金が始まるよ。",
                    "{provider} {kind:?}"
                );
                assert_eq!(selected.provider, provider);
            }
            // 要約は中立な文体なので手を加えない。
            let summary =
                select_valid_output(SummaryOutputKind::Summary, response(noisy, provider)).unwrap();
            assert_eq!(summary.text, noisy);
        }
        // Mock の種（定型文）は決定的な結果のまま変えない。
        let mock_seed = "（経済）って、結局どこが便利になるのかを見ると分かりやすそうだね。";
        let selected =
            select_valid_output(SummaryOutputKind::Comment, response(mock_seed, "mock")).unwrap();
        assert_eq!(selected.text, mock_seed);
    }

    #[test]
    fn polite_endings_from_real_ai_speech_become_yuuko_endings_but_summary_and_mock_do_not() {
        let polite = "新しい補助金が始まります。とても便利な制度です。";
        for provider in ["gemini", "local"] {
            for kind in [SummaryOutputKind::Explanation, SummaryOutputKind::Comment] {
                let selected = select_valid_output(kind, response(polite, provider)).unwrap();
                assert_eq!(
                    selected.text, "新しい補助金が始まるよ。とても便利な制度だよ。",
                    "{provider} {kind:?}"
                );
            }
            // 重いニュース（D122）でも同じ常体の語尾にし、「！」は落ち着いた形（「。」）にする。
            let calm = select_valid_output_in_tone(
                SummaryOutputKind::Comment,
                SpeechTone::Calm,
                "",
                response("2人が亡くなりました！", provider),
            )
            .unwrap();
            assert_eq!(calm.text, "2人が亡くなったよ。", "{provider}");
            // 要約は中立な文体なので手を加えない。
            let summary =
                select_valid_output(SummaryOutputKind::Summary, response(polite, provider))
                    .unwrap();
            assert_eq!(summary.text, polite);
        }
        // Mock の種（定型文）は決定的な結果のまま変えない。
        let selected =
            select_valid_output(SummaryOutputKind::Comment, response(polite, "mock")).unwrap();
        assert_eq!(selected.text, polite);
    }

    #[test]
    fn rude_address_and_repeats_from_real_ai_speech_are_removed_but_summary_and_mock_are_not() {
        let noisy = "ねえ、お前ら、新しい補助金が始まります。新しい補助金が始まるよ！";
        for provider in ["gemini", "local"] {
            for kind in [SummaryOutputKind::Explanation, SummaryOutputKind::Comment] {
                let selected = select_valid_output(kind, response(noisy, provider)).unwrap();
                assert_eq!(
                    selected.text, "新しい補助金が始まるよ。",
                    "{provider} {kind:?}"
                );
            }
            // 要約は中立な文体なので手を加えない。
            let summary =
                select_valid_output(SummaryOutputKind::Summary, response(noisy, provider)).unwrap();
            assert_eq!(summary.text, noisy);
        }
        // Mock の種（定型文）は決定的な結果のまま変えない。
        let selected =
            select_valid_output(SummaryOutputKind::Comment, response(noisy, "mock")).unwrap();
        assert_eq!(selected.text, noisy);
    }

    #[test]
    fn heavy_news_speech_from_real_ai_is_calmed_but_mock_and_summary_are_not() {
        // 重いニュース（D122）: 実AIの再説明・一言からは、絵文字・「！」・語尾の「～」も取り除く。
        let noisy = "わあ、2人が亡くなったんだって！！みんな気をつけようね～♪✨";
        for provider in ["gemini", "local"] {
            for kind in [SummaryOutputKind::Explanation, SummaryOutputKind::Comment] {
                let selected = select_valid_output_in_tone(
                    kind,
                    SpeechTone::Calm,
                    "",
                    response(noisy, provider),
                )
                .unwrap();
                assert_eq!(
                    selected.text, "2人が亡くなったんだって。みんな気をつけようね",
                    "{provider} {kind:?}"
                );
            }
            // 要約は中立な文体なので、重いニュースでも手を加えない。
            let summary = select_valid_output_in_tone(
                SummaryOutputKind::Summary,
                SpeechTone::Calm,
                "",
                response("台風で被害が出た！", provider),
            )
            .unwrap();
            assert_eq!(summary.text, "台風で被害が出た！");
        }
        // ふだんの口調では「！」「♪」を残す（従来どおり）。
        let normal = select_valid_output(
            SummaryOutputKind::Comment,
            response("新しいチップが出るんだね！楽しみ♪", "local"),
        )
        .unwrap();
        assert_eq!(normal.text, "新しいチップが出るんだね！楽しみ♪");
        // Mock の種は決定的な結果のまま変えない。
        let mock_seed = "落ち着いて受け止めたいね！";
        let mock = select_valid_output_in_tone(
            SummaryOutputKind::Comment,
            SpeechTone::Calm,
            "",
            response(mock_seed, "mock"),
        )
        .unwrap();
        assert_eq!(mock.text, mock_seed);
    }

    /// 実際の要約の Mock の種（`build_summary_seed`。定型文が抜粋の最終行と同じ行に続く）を Mock に渡したときの要約。
    fn mock_summary_from_seed(excerpt: &str, level: ExplanationLevel) -> String {
        let article = crate::domain::article::ArticleDetailDto {
            article_id: ARTICLE_ID.to_string(),
            title: "Deno が Cloudflare に参加".to_string(),
            source_name: "テスト".to_string(),
            original_url: "https://example.com/a".to_string(),
            published_at_text: String::new(),
            genre: "テクノロジー".to_string(),
            summary: None,
            excerpt: Some(excerpt.to_string()),
            yuuko_explanation: None,
            key_points: Vec::new(),
            focus_points: Vec::new(),
            yuuko_comment: None,
            is_favorite: false,
            keyword_candidates: Vec::new(),
            summary_state: Default::default(),
            tags: Vec::new(),
        };
        let seed_article = neutralize_seed_article(&article);
        let focus_points = build_focus_points(&seed_article, level);
        let seed = build_summary_seed(&seed_article, level, &focus_points);
        let root_dir = std::env::temp_dir().join("yuuko_summary_mock_seed_test");
        AiProviderService::new(&AppPaths::new(root_dir))
            .request_text(
                // アプリと同じく、実AIの入力は要約の入力（D167）、Mock の種は context で渡す。
                crate::domain::summary::AiRequest {
                    prompt_id: crate::domain::summary::SUMMARY_PROMPT_ID.to_string(),
                    input_text: super::build_summary_input(
                        &seed_article.title,
                        seed_article.excerpt.as_deref(),
                        None,
                    ),
                    context: Some(seed),
                },
                crate::domain::settings::AiProvider::Mock,
                level,
            )
            .unwrap()
            .text
    }

    #[test]
    fn mock_summary_from_real_seeds_leaves_out_english_tweet_sentences() {
        // 英語のツイート文が抜粋の最終行にあり、定型文がその行に続く形（URL は Mock が除く）。
        let excerpt = "Deno が Cloudflare に加わると発表した。\nWe are excited to announce that Deno is joining Cloudflare! https://t.co/hC5HOUbtt4 — Deno (@deno_land) October 9, 2026";
        for level in [
            ExplanationLevel::Simple,
            ExplanationLevel::Normal,
            ExplanationLevel::Detailed,
        ] {
            let summary = mock_summary_from_seed(excerpt, level);
            assert!(
                summary.starts_with("Deno が Cloudflare に加わると発表した。"),
                "{level:?}: {summary}"
            );
            for english in ["We are excited", "deno_land", "October", "http"] {
                assert!(!summary.contains(english), "{level:?}: {summary}");
            }
        }
    }

    #[test]
    fn mock_summary_from_real_seeds_keeps_english_only_articles() {
        // 英語だけの複数行の記事は、定型文があっても抜粋の文を捨てない（従来どおり先頭の文から作る）。
        let excerpt =
            "Deno is joining Cloudflare.\nThe runtime will stay open source.\nMore details soon.";
        for level in [
            ExplanationLevel::Simple,
            ExplanationLevel::Normal,
            ExplanationLevel::Detailed,
        ] {
            let summary = mock_summary_from_seed(excerpt, level);
            assert!(
                summary
                    .starts_with("Deno is joining Cloudflare. The runtime will stay open source."),
                "{level:?}: {summary}"
            );
        }
    }

    #[test]
    fn heavy_news_mock_seeds_are_calm_and_normal_seeds_are_unchanged() {
        let article = crate::domain::article::ArticleDetailDto {
            article_id: ARTICLE_ID.to_string(),
            title: "住宅で火災 2人死亡".to_string(),
            source_name: "テスト".to_string(),
            original_url: "https://example.com/a".to_string(),
            published_at_text: String::new(),
            genre: "社会".to_string(),
            summary: None,
            excerpt: None,
            yuuko_explanation: None,
            key_points: Vec::new(),
            focus_points: Vec::new(),
            yuuko_comment: None,
            is_favorite: false,
            keyword_candidates: Vec::new(),
            summary_state: Default::default(),
            tags: Vec::new(),
        };
        let focus = vec!["火災の原因".to_string()];
        for level in [
            ExplanationLevel::Simple,
            ExplanationLevel::Normal,
            ExplanationLevel::Detailed,
        ] {
            let normal_explanation =
                build_yuuko_explanation_seed(&article, level, &focus, SpeechTone::Normal);
            let calm_explanation =
                build_yuuko_explanation_seed(&article, level, &focus, SpeechTone::Calm);
            let normal_comment = build_yuuko_comment_seed(&article, level, SpeechTone::Normal);
            let calm_comment = build_yuuko_comment_seed(&article, level, SpeechTone::Calm);
            assert_ne!(normal_explanation, calm_explanation);
            assert_ne!(normal_comment, calm_comment);
            for calm in [&calm_explanation, &calm_comment] {
                assert!(calm.contains("落ち着いて"), "{calm}");
                for cheerful in ["！", "面白", "便利", "楽し"] {
                    assert!(!calm.contains(cheerful), "{calm}");
                }
            }
            // 決定的（同じ入力なら同じ文）。
            assert_eq!(
                calm_explanation,
                build_yuuko_explanation_seed(&article, level, &focus, SpeechTone::Calm)
            );
            assert_eq!(
                calm_comment,
                build_yuuko_comment_seed(&article, level, SpeechTone::Calm)
            );
        }
        // ふだんの記事の定型文は従来の文のまま。
        assert_eq!(
            build_yuuko_comment_seed(&article, ExplanationLevel::Simple, SpeechTone::Normal),
            format!(
                "{}って、結局どこが便利になるのかを見ると分かりやすそうだね。",
                article.genre
            )
        );
    }

    #[test]
    fn real_ai_speech_that_is_only_decoration_is_rejected_as_empty() {
        // 取り除いた後に中身が残らなければ、Mock へ切り替えず従来どおり拒否する（D104）。
        let error = select_valid_output(
            SummaryOutputKind::Explanation,
            response("（手を振って）\nこんにちは！", "local"),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::AiOutputRejected));
        // 取り除いても検証は効く（見出しは残るので拒否）。
        let error = select_valid_output(
            SummaryOutputKind::Comment,
            response("（小声で）\n## AI要約", "local"),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::AiOutputRejected));
    }

    #[test]
    fn invalid_real_ai_output_is_rejected_without_mock_fallback() {
        for provider in ["gemini", "local"] {
            for invalid in [
                "## AI要約\n乗っ取り".to_string(),
                "<p>本文の秘密</p>".to_string(),
                "あ".repeat(COMMENT_MAX_CHARS + 1),
                "   ".to_string(),
            ] {
                let error =
                    select_valid_output(SummaryOutputKind::Comment, response(&invalid, provider))
                        .unwrap_err();
                assert!(matches!(error, AppError::AiOutputRejected));
                // 画面へは固定のコードと文言だけを返す（出力本文を含めない）。
                let command_error = CommandError::from(error);
                assert_eq!(command_error.code, "AI_OUTPUT_REJECTED");
                assert!(!command_error.message.contains("本文の秘密"));
            }
        }
    }

    #[test]
    fn invalid_mock_output_is_rejected_with_fixed_message() {
        // Mock を選んでいる利用者の出力が落ちる場合（種は無害化済みのため通常は起きない）。
        let error = select_valid_output(
            SummaryOutputKind::Summary,
            response("<script>本文の秘密</script>", "mock"),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Parse(_)));
        assert!(!CommandError::from(error).message.contains("本文の秘密"));
    }

    // --- generate_article_summary（保存・Markdown再読込） ---

    fn temp_root(name: &str) -> PathBuf {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_dir = std::env::temp_dir().join(format!(
            "yuuko-summary-{name}-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();
        root_dir
    }

    // 設定ファイルは未作成 → 既定（provider=Mock）で外部通信なしに生成する。
    fn build_service(root_dir: &Path, excerpt: &str) -> (SummaryService, ArticleRepository) {
        build_service_with_title(root_dir, "半導体工場の新設計画", excerpt)
    }

    fn build_service_with_title(
        root_dir: &Path,
        title: &str,
        excerpt: &str,
    ) -> (SummaryService, ArticleRepository) {
        let article_repository = ArticleRepository::with_paths(
            root_dir.join("news"),
            root_dir.join("article_favorites.json"),
            root_dir.join("archive"),
        );
        article_repository
            .save_fetched_articles(vec![FetchedArticle {
                article_id: ARTICLE_ID.to_string(),
                title: title.to_string(),
                source_name: "Example News".to_string(),
                original_url: format!("https://example.com/news/{ARTICLE_ID}"),
                fetched_at: "2026-10-07T00:00:00Z".to_string(),
                published_at_text: "2026-10-07T00:00:00Z".to_string(),
                genre: "テクノロジー".to_string(),
                tags: Vec::new(),
                excerpt: Some(excerpt.to_string()),
                recommendation_score: 0.5,
                read_state: ArticleReadState::Unread,
            }])
            .unwrap();
        let service = SummaryService::new(
            AiProviderService::new(&AppPaths::new(root_dir.join("ai"))),
            article_repository.clone(),
            SettingsRepository::with_path(root_dir.join("settings.json")),
        );
        (service, article_repository)
    }

    fn params() -> GenerateArticleSummaryParams {
        GenerateArticleSummaryParams {
            article_id: ARTICLE_ID.to_string(),
        }
    }

    #[test]
    fn valid_generation_is_saved_and_markdown_reloads_with_intact_sections() {
        let root_dir = temp_root("valid");
        let (service, repository) =
            build_service(&root_dir, "新しい半導体工場の建設計画が発表されました。");

        let generated = service.generate_article_summary(params()).unwrap();

        // Markdown を再読込しても、各セクションが生成値どおりに分かれて読める。
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some(generated.summary.as_str()));
        assert_eq!(
            detail.yuuko_explanation.as_deref(),
            Some(generated.yuuko_explanation.as_str())
        );
        assert_eq!(
            detail.yuuko_comment.as_deref(),
            Some(generated.yuuko_comment.as_str())
        );
        assert_eq!(
            detail.excerpt.as_deref(),
            Some("新しい半導体工場の建設計画が発表されました。")
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_explanation_and_comment_are_in_yuuko_tone_without_internal_labels() {
        let root_dir = temp_root("tone");
        let (service, _) = build_service(&root_dir, "新しい半導体工場の建設計画が発表されました。");

        // 既定（provider=Mock・normal）で生成。出力検証（§12.5）を通って保存・返却される。
        let generated = service.generate_article_summary(params()).unwrap();

        assert!(
            generated.yuuko_explanation.contains("だよ")
                || generated.yuuko_explanation.contains("よ。")
        );
        assert!(!generated.yuuko_explanation.contains("です"));
        assert!(generated.yuuko_comment.ends_with("だね。"));
        for text in [&generated.yuuko_explanation, &generated.yuuko_comment] {
            for label in ["mock", "normal", "simple", "detailed"] {
                assert!(!text.contains(label), "must not show {label}: {text}");
            }
        }
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_explanation_and_comment_keep_the_template_texts() {
        // 実AIの入力を要約・要点に変えても、Mock の再説明・一言は従来の定型文のまま（決定的）。
        let root_dir = temp_root("mock-template");
        let (service, _) = build_service(&root_dir, "新しい半導体工場の建設計画が発表されました。");

        let generated = service.generate_article_summary(params()).unwrap();

        assert_eq!(
            generated.yuuko_explanation,
            "この記事は、テクノロジー を起点に読むと理解しやすいよ。特に 半導体工場の新設計画 の要点を確認する がどう現場や利用者に影響するかを見ると、話の流れが追いやすくなるね。"
        );
        assert_eq!(
            generated.yuuko_comment,
            "半導体工場の新設計画の話だけど、仕組みより『使った先で何が変わるか』に目を向けると面白そうだね。"
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn explanation_and_comment_get_the_summary_and_key_points_of_the_same_run() {
        // 要点 → 要約 → 再説明 → 感想の順に呼び（D167）、再説明・感想の実AI入力は同じ生成の要約・要点にする。
        // 定型文（Mock 用）は context にだけ入り、実AIの入力（input_text）には入らない。
        let root_dir = temp_root("speech-input");
        let excerpt = "新しい半導体工場の建設計画が発表されました。投資額は1兆円です。";
        let (service, _) = build_service(&root_dir, excerpt);

        let requests = RefCell::new(Vec::new());
        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::RejectFallback,
                |request, kind, _provider, _level| {
                    requests.borrow_mut().push((kind, request));
                    Ok(match kind {
                        SummaryOutputKind::Summary => output(kind, "工場が新設される。", "gemini"),
                        _ => output(kind, "工場", "gemini"),
                    })
                },
            )
            .unwrap();

        let requests = requests.into_inner();
        let kinds = requests.iter().map(|(kind, _)| *kind).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                SummaryOutputKind::Points,
                SummaryOutputKind::Summary,
                SummaryOutputKind::Explanation,
                SummaryOutputKind::Comment
            ]
        );
        // 要約の実AI入力は、タイトル・本文抜粋と、先に作った要点（D167）。Mock の種（本文抜粋＋定型文）は
        // context にだけ入る。
        let (_, summary_request) = &requests[1];
        assert_eq!(summary_request.prompt_id, SUMMARY_PROMPT_ID);
        assert_eq!(
            summary_request.input_text,
            format!(
                "タイトル: 半導体工場の新設計画\n本文抜粋:\n{excerpt}\n要点:\n・工場（要点1）\n・工場（要点2）"
            )
        );
        assert!(!summary_request.input_text.contains("要点を確認する"));
        let summary_mock_seed = summary_request.context.as_deref().unwrap();
        assert!(
            summary_mock_seed.starts_with(excerpt),
            "{summary_mock_seed}"
        );
        assert!(!summary_mock_seed.contains("要点1"), "{summary_mock_seed}");
        let expected_input =
            "タイトル: 半導体工場の新設計画\n要約: 工場が新設される。\n要点:\n・工場（要点1）\n・工場（要点2）";
        for (kind, request) in &requests[2..] {
            assert_eq!(request.input_text, expected_input, "{kind:?}");
            assert!(!request.input_text.contains(excerpt), "{kind:?}");
            assert!(!request.input_text.contains("要点を確認する"), "{kind:?}");
            assert!(!request.input_text.contains("面白そうだね"), "{kind:?}");
            // Mock 用の定型文は context にだけ入る（build_prompt は送らない）。
            assert!(request.context.as_deref().unwrap_or("").ends_with("ね。"));
        }
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    /// 要約の機械点検と1回の作り直し（D166）を、取得手段を差し替えて確かめる。
    /// `summaries` は要約の呼び出しごとの出力（最初・作り直し）。足りないときは Err を返す。
    fn generate_with_summaries(
        service: &SummaryService,
        summaries: Vec<Result<AiResponse, AppError>>,
    ) -> (
        Result<SummaryGeneration, AppError>,
        Vec<crate::domain::summary::AiRequest>,
    ) {
        let summaries = RefCell::new(summaries.into_iter());
        let requests = RefCell::new(Vec::new());
        let result = service.generate_article_summary_with(
            params(),
            FallbackPolicy::SaveFallback,
            |request, kind, _provider, _level| {
                let is_summary = kind == SummaryOutputKind::Summary;
                requests.borrow_mut().push(request);
                if is_summary {
                    return summaries
                        .borrow_mut()
                        .next()
                        .unwrap_or(Err(AppError::AiOutputRejected));
                }
                Ok(output(kind, "工場", "local"))
            },
        );
        (result, requests.into_inner())
    }

    fn generated_summary(result: Result<SummaryGeneration, AppError>) -> String {
        match result.unwrap() {
            SummaryGeneration::Generated(generated) => generated.summary,
            SummaryGeneration::AlreadySummarized => panic!("not generated"),
        }
    }

    const RETRY_EXCERPT: &str = "新しい半導体工場の建設計画が発表されました。投資額は1兆円です。";

    #[test]
    fn summary_with_an_unknown_number_is_regenerated_once_with_the_reason() {
        let root_dir = temp_root("retry-adopted");
        let (service, _) = build_service(&root_dir, RETRY_EXCERPT);

        let (result, requests) = generate_with_summaries(
            &service,
            vec![
                Ok(response("投資額2兆円の工場が新設される。", "local")),
                Ok(response("投資額1兆円の工場が新設される。", "local")),
            ],
        );

        assert_eq!(generated_summary(result), "投資額1兆円の工場が新設される。");
        let prompt_ids: Vec<&str> = requests.iter().map(|r| r.prompt_id.as_str()).collect();
        assert_eq!(
            prompt_ids,
            vec![
                ARTICLE_POINTS_PROMPT_ID,
                SUMMARY_PROMPT_ID,
                crate::domain::summary::SUMMARY_RETRY_PROMPT_ID,
                crate::domain::summary::YUUKO_EXPLANATION_PROMPT_ID,
                crate::domain::summary::YUUKO_COMMENT_PROMPT_ID,
            ]
        );
        // 作り直しは要点を外した入力（タイトル＋本文抜粋）から。理由（数字だけを含む固定の文）を context で渡す。
        assert!(requests[1].input_text.contains("・工場（要点1）"));
        assert_eq!(
            requests[2].input_text,
            format!("タイトル: 半導体工場の新設計画\n本文抜粋:\n{RETRY_EXCERPT}")
        );
        let reasons = requests[2].context.as_deref().unwrap();
        assert!(reasons.contains("「2兆」は記事情報にない数字"), "{reasons}");
        // 再説明・一言の入力は、採った要約を使う。
        assert!(requests[3].input_text.contains("投資額1兆円"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn first_summary_is_kept_when_the_regeneration_is_not_better_or_fails() {
        let first = "投資額2兆円の工場が新設される。";
        let cases: Vec<Result<AiResponse, AppError>> = vec![
            // 同じ問題が残る。
            Ok(response("投資額3兆円の工場が新設される。", "local")),
            // 数字は直っても、最初になかった種類の問題（簡体字）が出る。
            Ok(response("新しい工场が建つ。", "local")),
            // 作り直しが検証に落ちた・失敗した。
            Err(AppError::AiOutputRejected),
            // Gemini の失敗で Mock に切り替わった（Mock の結果は使わない）。
            Ok(response("工場が新設される。", "mock")),
        ];
        for (index, retried) in cases.into_iter().enumerate() {
            let root_dir = temp_root(&format!("retry-kept-{index}"));
            let (service, _) = build_service(&root_dir, RETRY_EXCERPT);
            let (result, requests) =
                generate_with_summaries(&service, vec![Ok(response(first, "local")), retried]);
            assert_eq!(generated_summary(result), first, "case {index}");
            assert_eq!(requests.len(), 5, "case {index}");
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    #[test]
    fn summary_without_issues_or_from_mock_is_not_regenerated() {
        for (index, summary) in [
            // 問題なし（数字も記事情報どおり）。
            response("投資額1兆円の工場が新設される。", "local"),
            // 注記は作り直しの理由にしない（誤検出しやすいため）。
            response("工場が新設される。時期は記事には書かれていない。", "gemini"),
            // Mock（本文の抜粋）は点検しない。
            response("投資額は9兆円です。", "mock"),
        ]
        .into_iter()
        .enumerate()
        {
            let root_dir = temp_root(&format!("retry-none-{index}"));
            let (service, _) = build_service(&root_dir, RETRY_EXCERPT);
            let expected = summary.text.clone();
            let (result, requests) = generate_with_summaries(&service, vec![Ok(summary)]);
            assert_eq!(generated_summary(result), expected, "case {index}");
            assert_eq!(requests.len(), 4, "case {index}");
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    // --- D170: 見出しの残り・反対の意味・年の組み合わせの点検 ---

    /// 段ごとに決めた出力を順に返す取得手段で要約一式を作る（実AIと同じく保存前の検証・後処理を通す）。
    /// 用意した出力が尽きた段は検証落ち（AiOutputRejected）にする。
    struct Script {
        points: Vec<String>,
        summary: Vec<&'static str>,
        explanation: Vec<&'static str>,
        comment: Vec<&'static str>,
    }

    fn generate_scripted(
        service: &SummaryService,
        script: Script,
    ) -> (
        Result<SummaryGeneration, AppError>,
        Vec<crate::domain::summary::AiRequest>,
    ) {
        let points = RefCell::new(script.points.into_iter());
        let summary = RefCell::new(script.summary.into_iter());
        let explanation = RefCell::new(script.explanation.into_iter());
        let comment = RefCell::new(script.comment.into_iter());
        let requests = RefCell::new(Vec::new());
        let result = service.generate_article_summary_with(
            params(),
            FallbackPolicy::SaveFallback,
            |request, kind, _provider, _level| {
                requests.borrow_mut().push(request);
                let next = match kind {
                    SummaryOutputKind::Points => points.borrow_mut().next(),
                    SummaryOutputKind::Summary => summary.borrow_mut().next().map(str::to_string),
                    SummaryOutputKind::Explanation => {
                        explanation.borrow_mut().next().map(str::to_string)
                    }
                    SummaryOutputKind::Comment => comment.borrow_mut().next().map(str::to_string),
                };
                match next {
                    Some(text) => select_valid_output(kind, response(&text, "local")),
                    None => Err(AppError::AiOutputRejected),
                }
            },
        );
        (result, requests.into_inner())
    }

    fn generated(result: Result<SummaryGeneration, AppError>) -> GeneratedArticleSummaryDto {
        match result.unwrap() {
            SummaryGeneration::Generated(generated) => generated,
            SummaryGeneration::AlreadySummarized => panic!("not generated"),
        }
    }

    /// 架空のチームの試合（D169 の A2 の形を、架空の名前と文で書いたもの）。
    const LOSS_TITLE: &str = "みなとシーガルズ、最終回にサヨナラ負け";
    const LOSS_EXCERPT: &str =
        "みなとシーガルズは9回裏に逆転打を許してやまびこベアーズに敗れた。青木は3安打だった。";

    fn loss_points() -> String {
        points_text(
            &[
                "みなとシーガルズはやまびこベアーズにサヨナラ負けした",
                "青木は3安打だった",
            ],
            &["救援陣の失点に注目"],
        )
    }

    #[test]
    fn summary_that_reverses_the_article_is_regenerated_once_with_the_reason() {
        let root_dir = temp_root("d170-summary-retry");
        let (service, _) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![loss_points()],
                summary: vec![
                    "みなとシーガルズはサヨナラ勝ちした。",
                    "みなとシーガルズはやまびこベアーズにサヨナラ負けした。",
                ],
                explanation: vec!["みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["悔しい負けだね"],
            },
        );
        assert_eq!(
            generated(result).summary,
            "みなとシーガルズはやまびこベアーズにサヨナラ負けした。"
        );
        assert_eq!(requests.len(), 5);
        assert_eq!(
            requests[2].prompt_id,
            crate::domain::summary::SUMMARY_RETRY_PROMPT_ID
        );
        let reasons = requests[2].context.as_deref().unwrap();
        assert!(reasons.contains("「勝ち」"), "{reasons}");
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn summary_that_stays_reversed_after_the_regeneration_is_not_saved() {
        let root_dir = temp_root("d170-summary-rejected");
        let (service, repository) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![loss_points()],
                summary: vec![
                    "みなとシーガルズはサヨナラ勝ちした。",
                    "みなとシーガルズはやまびこベアーズに勝利した。",
                ],
                explanation: vec!["みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["悔しい負けだね"],
            },
        );
        assert!(matches!(result, Err(AppError::AiOutputRejected)));
        // 要点・要約・作り直しまでで止まり、再説明・一言は呼ばない。
        assert_eq!(requests.len(), 3);
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.yuuko_comment, None);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn regenerated_summary_with_fewer_issues_but_still_reversed_is_not_adopted() {
        // 最初: 反対の意味＋材料にない数字（2件）。作り直し: 反対の意味だけ（1件）。数は減っても採らず、保存しない。
        let root_dir = temp_root("d170-summary-fewer-but-reversed");
        let (service, repository) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![loss_points()],
                summary: vec![
                    "みなとシーガルズは5点差でサヨナラ勝ちした。",
                    "みなとシーガルズはサヨナラ勝ちした。",
                ],
                explanation: vec!["みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["悔しい負けだね"],
            },
        );
        assert!(matches!(result, Err(AppError::AiOutputRejected)));
        assert_eq!(requests.len(), 3);
        let reasons = requests[2].context.as_deref().unwrap();
        assert!(reasons.contains("「5」"), "{reasons}");
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.yuuko_comment, None);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn summary_with_a_year_paired_with_another_event_is_regenerated() {
        let root_dir = temp_root("d170-summary-year");
        let excerpt =
            "2023年12月にうみかぜ球団と10年の大型契約を結んだ。2026年に900万ドルの年俸を受け取った。";
        let (service, _) = build_service_with_title(&root_dir, "青山投手の年俸", excerpt);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![points_text(
                    &["2023年12月に大型契約を結んだ", "2026年の年俸は900万ドル"],
                    &["年俸の上がり方に注目"],
                )],
                summary: vec![
                    "青山投手は2026年にうみかぜ球団と大型契約を結んだ。",
                    "青山投手は2023年12月にうみかぜ球団と大型契約を結んだ。",
                ],
                explanation: vec!["2023年に契約したんだね。"],
                comment: vec!["すごい契約だね"],
            },
        );
        assert_eq!(
            generated(result).summary,
            "青山投手は2023年12月にうみかぜ球団と大型契約を結んだ。"
        );
        assert_eq!(requests.len(), 5);
        assert!(requests[2]
            .context
            .as_deref()
            .unwrap()
            .contains("「2026年」と「契約」"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn bold_heading_in_the_summary_is_stripped_without_regeneration() {
        let root_dir = temp_root("d170-heading");
        let (service, _) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![loss_points()],
                summary: vec!["**中心となる出来事の要約:** みなとシーガルズはやまびこベアーズにサヨナラ負けした。"],
                explanation: vec!["再説明：みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["**ゆうこの一言**\n悔しい負けだね"],
            },
        );
        let generated = generated(result);
        assert_eq!(
            generated.summary,
            "みなとシーガルズはやまびこベアーズにサヨナラ負けした。"
        );
        assert_eq!(
            generated.yuuko_explanation,
            "みなとシーガルズはサヨナラ負けしたんだね。"
        );
        assert_eq!(generated.yuuko_comment, "悔しい負けだね");
        assert_eq!(requests.len(), 4);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn leading_connectors_are_removed_from_real_ai_summary_and_speech() {
        // 2文目以降の文頭のつなぎの語と読点だけを消す（1文目・要点は触らない）。作り直しはしない。
        let root_dir = temp_root("leading-connector");
        let (service, _) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![loss_points()],
                summary: vec![
                    "みなとシーガルズはやまびこベアーズにサヨナラ負けした。その結果、青木は3安打だった。",
                ],
                explanation: vec![
                    "このため、みなとシーガルズは負けたんだね。これにより、青木は3安打だったよ。",
                ],
                comment: vec!["サヨナラ負けだね。そのため、悔しいね"],
            },
        );
        let generated = generated(result);
        assert_eq!(
            generated.summary,
            "みなとシーガルズはやまびこベアーズにサヨナラ負けした。青木は3安打だった。"
        );
        assert_eq!(
            generated.yuuko_explanation,
            "このため、みなとシーガルズは負けたんだね。青木は3安打だったよ。"
        );
        assert_eq!(generated.yuuko_comment, "サヨナラ負けだね。悔しいね");
        assert_eq!(requests.len(), 4);
        // 再説明・一言の入力の要約も、消した後の文。
        assert!(
            requests[2].input_text.contains(
                "要約: みなとシーガルズはやまびこベアーズにサヨナラ負けした。青木は3安打だった。"
            ),
            "{}",
            requests[2].input_text
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn leading_connectors_are_removed_from_the_regenerated_summary_too() {
        // 作り直しも同じ取得手段を通るので、作り直した要約のつなぎの語も消える。
        let root_dir = temp_root("leading-connector-retry");
        let (service, _) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![loss_points()],
                summary: vec![
                    "みなとシーガルズはやまびこベアーズにサヨナラ勝ちした。",
                    "みなとシーガルズはやまびこベアーズにサヨナラ負けした。その結果、青木は3安打だった。",
                ],
                explanation: vec!["みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["悔しい負けだね"],
            },
        );
        assert_eq!(
            generated(result).summary,
            "みなとシーガルズはやまびこベアーズにサヨナラ負けした。青木は3安打だった。"
        );
        assert_eq!(requests.len(), 5);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn leading_connectors_are_kept_in_mock_outputs() {
        // Mock の結果（本文の抜粋・定型文）は触らない。記事に「このため」がない本文でも、Mock の要約はそのまま。
        let response = AiResponse {
            text: "台風が来た。このため、中止した。".to_string(),
            provider: "mock".to_string(),
        };
        let kept = strip_ai_leading_connectors(
            SummaryOutputKind::Summary,
            response.clone(),
            "台風が来た。",
        );
        assert_eq!(kept.text, response.text);
        // 要点の段も触らない。
        let points = AiResponse {
            text: response.text.clone(),
            provider: "local".to_string(),
        };
        assert_eq!(
            strip_ai_leading_connectors(SummaryOutputKind::Points, points, "台風が来た。").text,
            response.text
        );
        // 実AIの要約は消す。
        let stripped = strip_ai_leading_connectors(
            SummaryOutputKind::Summary,
            AiResponse {
                text: response.text,
                provider: "local".to_string(),
            },
            "台風が来た。",
        );
        assert_eq!(stripped.text, "台風が来た。中止した。");
    }

    #[test]
    fn reversed_comment_is_regenerated_once_then_replaced_by_the_fixed_text() {
        // 作り直しで直れば作り直した一言を使う。
        let root_dir = temp_root("d170-comment-adopted");
        let (service, _) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![loss_points()],
                summary: vec!["みなとシーガルズはやまびこベアーズにサヨナラ負けした。"],
                explanation: vec!["みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["みなとシーガルズはサヨナラ勝ちしたんだね", "悔しい負けだね"],
            },
        );
        assert_eq!(generated(result).yuuko_comment, "悔しい負けだね");
        assert_eq!(requests.len(), 5);
        // 作り直しは同じ依頼（入力・定型文の種が同じ）。
        assert_eq!(requests[3].input_text, requests[4].input_text);
        let _ = std::fs::remove_dir_all(&root_dir);

        // 作り直しても反対の意味なら、定型文（Mock と同じ文）を使い、誤った一言は見せない。
        let root_dir = temp_root("d170-comment-template");
        let (service, repository) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![loss_points()],
                summary: vec!["みなとシーガルズはやまびこベアーズにサヨナラ負けした。"],
                explanation: vec![
                    "みなとシーガルズはサヨナラ勝ちしたんだね。",
                    "みなとシーガルズの勝利だね。",
                ],
                comment: vec!["悔しい負けだね"],
            },
        );
        let generated = generated(result);
        let template = requests[2].context.clone().unwrap();
        assert_eq!(generated.yuuko_explanation, template);
        assert!(!generated.yuuko_explanation.contains("勝"));
        assert_eq!(requests.len(), 5);
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.yuuko_explanation.as_deref(), Some(template.as_str()));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn reversed_key_points_are_dropped_and_regenerated_only_when_too_few_remain() {
        // 外しても 2 件残るなら作り直さない。
        let root_dir = temp_root("d170-points-dropped");
        let (service, _) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![points_text(
                    &[
                        "みなとシーガルズはやまびこベアーズにサヨナラ負けした",
                        "みなとシーガルズは最後にサヨナラ勝ちした",
                        "青木は3安打だった",
                    ],
                    &["救援陣の失点に注目"],
                )],
                summary: vec!["みなとシーガルズはやまびこベアーズにサヨナラ負けした。"],
                explanation: vec!["みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["悔しい負けだね"],
            },
        );
        assert_eq!(
            generated(result).key_points,
            vec![
                "みなとシーガルズはやまびこベアーズにサヨナラ負けした",
                "青木は3安打だった"
            ]
        );
        assert_eq!(requests.len(), 4);
        let _ = std::fs::remove_dir_all(&root_dir);

        // 足りなくなったら1回だけ作り直す。
        let root_dir = temp_root("d170-points-retry");
        let (service, _) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![
                    points_text(
                        &[
                            "みなとシーガルズは最後にサヨナラ勝ちした",
                            "青木は3安打だった",
                        ],
                        &["救援陣の失点に注目"],
                    ),
                    loss_points(),
                ],
                summary: vec!["みなとシーガルズはやまびこベアーズにサヨナラ負けした。"],
                explanation: vec!["みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["悔しい負けだね"],
            },
        );
        assert_eq!(
            generated(result).key_points,
            vec![
                "みなとシーガルズはやまびこベアーズにサヨナラ負けした",
                "青木は3安打だった"
            ]
        );
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[0].input_text, requests[1].input_text);
        let _ = std::fs::remove_dir_all(&root_dir);

        // 作り直しても足りなければ保存しない。
        let root_dir = temp_root("d170-points-rejected");
        let (service, _) = build_service_with_title(&root_dir, LOSS_TITLE, LOSS_EXCERPT);
        let bad = points_text(
            &[
                "みなとシーガルズは最後にサヨナラ勝ちした",
                "青木は3安打だった",
            ],
            &["救援陣の失点に注目"],
        );
        let (result, requests) = generate_scripted(
            &service,
            Script {
                points: vec![bad.clone(), bad],
                summary: vec!["みなとシーガルズはやまびこベアーズにサヨナラ負けした。"],
                explanation: vec!["みなとシーガルズはサヨナラ負けしたんだね。"],
                comment: vec!["悔しい負けだね"],
            },
        );
        assert!(matches!(result, Err(AppError::AiOutputRejected)));
        assert_eq!(requests.len(), 2);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_stops_when_a_manual_summary_starts_before_the_regeneration() {
        let root_dir = temp_root("retry-preempted");
        let (service, _) = build_service(&root_dir, RETRY_EXCERPT);
        let guard = RefCell::new(None);
        let calls = RefCell::new(0);
        let waits = RefCell::new(0);
        let outcome = service
            .generate_auto_summary_with(
                params(),
                |_provider| {
                    *waits.borrow_mut() += 1;
                    true
                },
                |_request, kind, _provider, _level| {
                    *calls.borrow_mut() += 1;
                    // 最初の要約を返した直後に、利用者が同じ記事の手動要約を始める。
                    if kind == SummaryOutputKind::Summary {
                        guard.replace(Some(service.mark_manual_summary_in_progress(ARTICLE_ID)));
                    }
                    Ok(output(kind, "投資額2兆円の工場が新設される。", "local"))
                },
            )
            .unwrap();
        assert_eq!(outcome, super::AutoSummaryOutcome::YieldedToManual);
        // 作り直しも、その後の段も呼ばない。作り直しの停止をそのまま返すので、再説明の段の待ちにも進まない
        // （待ちは要点・最初の要約・作り直しの3回だけ）。
        assert_eq!(*calls.borrow(), 2);
        assert_eq!(*waits.borrow(), 3);
        drop(guard);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_stops_when_the_app_exits_before_the_regeneration() {
        let root_dir = temp_root("retry-exit");
        let (service, repository) = build_service(&root_dir, RETRY_EXCERPT);
        let waits = RefCell::new(0);
        let calls = RefCell::new(0);
        let result = service.generate_auto_summary_with(
            params(),
            // 作り直しの前の待ち（要点・最初の要約の後の3回目）でアプリの終了に起こされる。
            |_provider| {
                *waits.borrow_mut() += 1;
                *waits.borrow() <= 2
            },
            |_request, kind, _provider, _level| {
                *calls.borrow_mut() += 1;
                Ok(output(kind, "投資額2兆円の工場が新設される。", "local"))
            },
        );
        assert!(matches!(result, Err(AppError::Validation(_))), "{result:?}");
        assert_eq!(*calls.borrow(), 2);
        assert_eq!(*waits.borrow(), 3);
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_stops_when_the_regeneration_falls_back_to_mock() {
        let root_dir = temp_root("retry-auto-mock");
        let (service, repository) = build_service(&root_dir, RETRY_EXCERPT);
        let requests = RefCell::new(Vec::new());
        let result = service.generate_auto_summary_with(
            params(),
            |_provider| true,
            |request, kind, _provider, _level| {
                let retry = request.prompt_id == crate::domain::summary::SUMMARY_RETRY_PROMPT_ID;
                requests.borrow_mut().push(request.prompt_id);
                Ok(if retry {
                    output(kind, "工場が新設される。", "mock")
                } else {
                    output(kind, "投資額2兆円の工場が新設される。", "gemini")
                })
            },
        );
        // Mock の結果は保存せず、再説明以降の AI 呼び出しもしない（D56）。
        assert!(matches!(result, Err(AppError::Network(_))), "{result:?}");
        assert_eq!(requests.borrow().len(), 3);
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn regeneration_with_a_new_kind_of_issue_is_not_better_even_with_fewer_issues() {
        let source = "対象は28校。";
        let check =
            |text: &str| super::output_check::check_japanese_summary(source, text, None, &[]);
        // 最初: 材料にない数字が2つ。
        let first = check("対象は35校と40校。");
        assert_eq!(first.issues.len(), 2);
        // 作り直し: 数字はないが、簡体字が1つ（件数は減るが、最初になかった種類）。
        let new_kind = check("対象の学校で电池を使う。");
        assert_eq!(new_kind.issues.len(), 1);
        assert!(!super::is_retry_better(&first, &new_kind));
        // 同じ種類で件数が減ったときは採る。
        let fewer = check("対象は28校と40校。");
        assert!(super::is_retry_better(&first, &fewer));
        // 件数が同じなら採らない。
        assert!(!super::is_retry_better(
            &first,
            &check("対象は36校と41校。")
        ));
    }

    #[test]
    fn strips_site_name_and_category_suffix_from_seed_title() {
        let cases = [
            (
                "高市首相が忘れた第1次安倍政権の教訓 | 政治・経済・投資 | 東洋経済オンライン",
                "東洋経済オンライン",
                "高市首相が忘れた第1次安倍政権の教訓",
            ),
            (
                "Microsoft、第3世代AIチップを2027年に投入 - ITmedia NEWS",
                "ITmedia NEWS 最新記事一覧",
                "Microsoft、第3世代AIチップを2027年に投入",
            ),
            (
                "台風13号 影響は長時間続く 警戒を｜NHKニュース",
                "NHKニュース|社会",
                "台風13号 影響は長時間続く 警戒を",
            ),
            (
                "大谷翔平がスタメン外の可能性 - Full-Count（フルカウント）",
                "Full-Count（フルカウント） ― 野球ニュース・速報・コラム ―",
                "大谷翔平がスタメン外の可能性",
            ),
            (
                "新しい半導体工場の建設計画を発表 | Example News",
                "example  news",
                "新しい半導体工場の建設計画を発表",
            ),
            (
                "新しい半導体工場の建設計画を発表：Example News",
                "Example News",
                "新しい半導体工場の建設計画を発表",
            ),
        ];
        for (title, source, expected) in cases {
            assert_eq!(
                seed_title_without_site_suffix(title, source),
                expected,
                "{title}"
            );
        }
    }

    #[test]
    fn keeps_seed_title_when_suffix_is_not_the_source_site() {
        let cases = [
            // 末尾が出典名でない（連載名・人名・製品名）。
            (
                "ロイヤルホストの新作メニュー《実食レビュー》 - 今日のリーマンめし!!",
                "ダイヤモンド・オンライン - 新着記事",
            ),
            ("ドジャースが逆転負け - 大谷翔平", "Full-Count"),
            ("新モデルを発表：GPT-5", "Example News"),
            ("Wi-Fi 7対応ルーターが登場", "Wi-Fi Times"),
            // 出典名の語の途中で切れる一致・短い語の前方一致は外さない。
            ("OpenAIが新しい推論モデルを公開 | Tech", "TechCrunch Japan"),
            ("OpenAIが新しい推論モデルを公開 | AI", "AI News Japan"),
            // 出典名の残りが固有の語（「Switch」「宇宙へのポータルサイト」）なら前方一致させない。
            ("ゲームの新作を発表 - Nintendo", "Nintendo Switch"),
            (
                "渦巻腕が描く“目”のような銀河の姿 | sorae",
                "sorae 宇宙へのポータルサイト",
            ),
            // `：` の後ろは前方一致させない（記事の主語・対象であることが多い）。
            ("ゲームの新作を発表：Nintendo", "Nintendo 公式"),
            // 外すと短くなりすぎる。
            ("速報 | Example News", "Example News"),
            // 出典名が空（取得元不明）。
            ("新しい半導体工場の建設計画を発表 | Example News", ""),
        ];
        for (title, source) in cases {
            assert_eq!(
                seed_title_without_site_suffix(title, source),
                title,
                "{title}"
            );
        }
    }

    #[test]
    fn keeps_short_head_instead_of_treating_it_as_category() {
        // 前半が短いときは、2 つ目の区切りの後ろを本文のタイトルとみなしてカテゴリ扱いしない。
        assert_eq!(
            seed_title_without_site_suffix("【連載】第3回｜AIの未来｜Example News", "Example News"),
            "【連載】第3回｜AIの未来"
        );
        // 区切りの記号が違うものはカテゴリとみなさない。
        assert_eq!(
            seed_title_without_site_suffix(
                "新しい半導体工場の建設計画を発表 - 解説 | Example News",
                "Example News"
            ),
            "新しい半導体工場の建設計画を発表 - 解説"
        );
    }

    #[test]
    fn seed_title_drops_site_suffix_but_saved_title_is_unchanged() {
        // サイト名を外すのは AI の種だけ。保存するタイトル（表示・重複判定のキー）は変えない。
        let root_dir = temp_root("seed-title-site");
        let title = "半導体工場の新設計画が始動 | テクノロジー | Example News";
        let excerpt = "新しい半導体工場の建設計画が発表されました。";
        let (service, repository) = build_service_with_title(&root_dir, title, excerpt);

        let requests = RefCell::new(Vec::new());
        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::RejectFallback,
                |request, kind, _provider, _level| {
                    requests.borrow_mut().push((kind, request));
                    Ok(match kind {
                        SummaryOutputKind::Summary => output(kind, "工場が新設される。", "gemini"),
                        _ => output(kind, "工場", "gemini"),
                    })
                },
            )
            .unwrap();

        let requests = requests.into_inner();
        let points_input = &requests[0].1.input_text;
        assert_eq!(
            points_input,
            &format!(
                "半導体工場の新設計画が始動
{excerpt}"
            )
        );
        for (kind, request) in &requests[2..] {
            assert!(
                request.input_text.starts_with(
                    "タイトル: 半導体工場の新設計画が始動
"
                ),
                "{kind:?}"
            );
        }
        for (kind, request) in &requests {
            assert!(!request.input_text.contains("Example News"), "{kind:?}");
        }
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.title, title);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_summary_is_left_out_of_the_explanation_and_comment_input() {
        // 手動生成で要約だけ Mock の代替になったとき、本文の短い抜粋の要約は実AIの入力に入れず、
        // タイトルと要点だけを渡す。
        let root_dir = temp_root("speech-input-mock-summary");
        let excerpt = "新しい半導体工場の建設計画が発表されました。";
        let (service, _) = build_service(&root_dir, excerpt);

        let requests = RefCell::new(Vec::new());
        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |request, kind, _provider, _level| {
                    requests.borrow_mut().push((kind, request));
                    Ok(match kind {
                        SummaryOutputKind::Summary => output(kind, "代わりの要約。", "mock"),
                        _ => output(kind, "工場", "gemini"),
                    })
                },
            )
            .unwrap();

        let requests = requests.into_inner();
        let expected_input =
            "タイトル: 半導体工場の新設計画\n要点:\n・工場（要点1）\n・工場（要点2）";
        for (kind, request) in &requests[2..] {
            assert_eq!(request.input_text, expected_input, "{kind:?}");
            assert!(!request.input_text.contains("代わりの要約"), "{kind:?}");
        }
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_key_points_are_left_out_of_the_summary_input() {
        // 手動生成で要点だけ Mock の代替（本文の文と定型文）になったとき、要約の実AI入力には要点を入れず、
        // タイトルと本文抜粋だけを渡す（D167）。Mock の要点は従来どおり保存される。
        let root_dir = temp_root("summary-input-mock-points");
        let excerpt = "新しい半導体工場の建設計画が発表されました。";
        let (service, repository) = build_service(&root_dir, excerpt);

        let requests = RefCell::new(Vec::new());
        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |request, kind, _provider, _level| {
                    requests.borrow_mut().push((kind, request));
                    Ok(match kind {
                        SummaryOutputKind::Points => output(kind, "代わりの要点", "mock"),
                        _ => output(kind, "工場が新設される。", "gemini"),
                    })
                },
            )
            .unwrap();

        let requests = requests.into_inner();
        let (kind, summary_request) = &requests[1];
        assert_eq!(*kind, SummaryOutputKind::Summary);
        assert_eq!(
            summary_request.input_text,
            format!("タイトル: 半導体工場の新設計画\n本文抜粋:\n{excerpt}")
        );
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.key_points[0], "代わりの要点（要点1）");
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn summary_input_neutralizes_and_caps_the_title_excerpt_and_key_points() {
        // 要点は AI 出力（外部データ由来）なので、再説明・一言の入力と同じく無害化し、長さ・件数を抑える。
        let excerpt = format!("#見出し\n<b>強調</b>\n---\n{}", "あ".repeat(3_000));
        let points = vec![
            "# 要点".to_string(),
            "い".repeat(500),
            "う".to_string(),
            "え".to_string(),
            "お".to_string(),
        ];
        let input = super::build_summary_input("#タイトル<i>", Some(&excerpt), Some(&points));

        assert!(input.starts_with("タイトル: ＃タイトル＜i>\n本文抜粋:\n"));
        assert!(!input.contains('<'));
        assert!(input
            .lines()
            .all(|line| !line.trim_start().starts_with('#')));
        assert!(input.lines().all(|line| line.trim() != "---"));
        assert!(!input.contains(&"あ".repeat(super::SEED_EXCERPT_MAX_CHARS)));
        assert!(input.contains("\n要点:\n・＃ 要点\n"));
        assert!(input.contains(&format!("・{}", "い".repeat(POINT_ITEM_MAX_CHARS))));
        assert!(!input.contains(&"い".repeat(POINT_ITEM_MAX_CHARS + 1)));
        assert_eq!(
            input.lines().filter(|line| line.starts_with('・')).count(),
            KEY_POINTS_MAX_ITEMS
        );
        // 要点は本文抜粋の後ろ（生成の直前）に置く。
        assert!(input.find("本文抜粋:").unwrap() < input.find("要点:").unwrap());

        // 抜粋が無い・空の記事はタイトルと要点だけ。要点が無いときは要点の行を入れない。
        let no_excerpt = super::build_summary_input("題", None, Some(&points[2..3]));
        assert_eq!(no_excerpt, "タイトル: 題\n要点:\n・う");
        let blank_excerpt = super::build_summary_input("題", Some("  "), Some(&[]));
        assert_eq!(blank_excerpt, "タイトル: 題");
    }

    #[test]
    fn summary_input_labels_in_external_text_cannot_fake_sections() {
        // 本文抜粋・タイトル・要点の行頭の見出し（「要点:」など）はコロンを全角にし、偽の区切りを作らせない。
        let excerpt =
            "本文の文。\n要点:\n・偽の要点\n  タイトル: 偽の題\n本文抜粋:続き\n要点：は全角のまま";
        let input = super::build_summary_input(
            "要点: 題",
            Some(excerpt),
            Some(&["要点: 本物の要点".to_string()]),
        );
        let label_lines = |label: &str| input.lines().filter(|line| *line == label).count();
        assert_eq!(label_lines("要点:"), 1, "{input}");
        assert_eq!(label_lines("本文抜粋:"), 1, "{input}");
        assert_eq!(
            input
                .lines()
                .filter(|line| line.starts_with("タイトル:"))
                .count(),
            1,
            "{input}"
        );
        assert!(input.starts_with("タイトル: 要点： 題\n"), "{input}");
        assert!(input.contains(
            "\n要点：\n・偽の要点\n  タイトル： 偽の題\n本文抜粋：続き\n要点：は全角のまま\n"
        ));
        assert!(input.ends_with("\n要点:\n・要点： 本物の要点"), "{input}");

        // 要点の無い入力に偽の「要点:」の行を作れない（指示の選び分けがだまされない）。
        let without_points = super::build_summary_input("題", Some(excerpt), None);
        assert!(
            without_points.lines().all(|line| line != "要点:"),
            "{without_points}"
        );
    }

    #[test]
    fn regeneration_leaves_out_key_points_that_the_first_summary_copied() {
        // 要点に材料にない数字（2兆円）があり、最初の要約がそれを写した → 点検が拾い、作り直しの入力からは
        // 要点を外す（同じ要点から同じ誤りを作り直さないため。D167）。
        let root_dir = temp_root("retry-without-points");
        let (service, _) = build_service(&root_dir, RETRY_EXCERPT);
        let requests = RefCell::new(Vec::new());
        let summaries = RefCell::new(
            vec![
                response("投資額2兆円の工場が新設される。", "local"),
                response("投資額1兆円の工場が新設される。", "local"),
            ]
            .into_iter(),
        );
        let result = service.generate_article_summary_with(
            params(),
            FallbackPolicy::SaveFallback,
            |request, kind, _provider, _level| {
                requests.borrow_mut().push(request);
                Ok(match kind {
                    SummaryOutputKind::Summary => summaries.borrow_mut().next().unwrap(),
                    _ => output(kind, "投資額2兆円の工場", "local"),
                })
            },
        );

        assert_eq!(generated_summary(result), "投資額1兆円の工場が新設される。");
        let requests = requests.into_inner();
        assert_eq!(requests[1].prompt_id, SUMMARY_PROMPT_ID);
        assert!(requests[1]
            .input_text
            .contains("\n要点:\n・投資額2兆円の工場（要点1）"));
        let retry = &requests[2];
        assert_eq!(
            retry.prompt_id,
            crate::domain::summary::SUMMARY_RETRY_PROMPT_ID
        );
        assert!(!retry.input_text.contains("要点"), "{}", retry.input_text);
        assert!(!retry.input_text.contains("2兆"), "{}", retry.input_text);
        assert_eq!(
            retry.input_text,
            format!("タイトル: 半導体工場の新設計画\n本文抜粋:\n{RETRY_EXCERPT}")
        );
        assert!(retry
            .context
            .as_deref()
            .unwrap()
            .contains("「2兆」は記事情報にない数字"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn yuuko_speech_input_neutralizes_and_caps_the_ai_outputs() {
        // 要約・要点は AI 出力（外部データ由来）なので、種と同じく行頭 `#`・`<`・区切り行を無害化し、長さを抑える。
        let summary = format!("#見出し\n<b>強調</b>\n---\n{}", "あ".repeat(1_000));
        let long_point = "い".repeat(500);
        let points = vec!["# 要点".to_string(), long_point];
        let input = build_yuuko_speech_input("#タイトル<i>", Some(&summary), &points);

        assert!(input.starts_with("タイトル: ＃タイトル＜i>"));
        assert!(!input.contains('<'));
        assert!(input
            .lines()
            .all(|line| !line.trim_start().starts_with('#')));
        assert!(input.lines().all(|line| line.trim() != "---"));
        assert!(input.contains("\n・＃ 要点\n"));
        assert!(input.contains(&format!("・{}", "い".repeat(POINT_ITEM_MAX_CHARS))));
        assert!(!input.contains(&"い".repeat(POINT_ITEM_MAX_CHARS + 1)));
        assert!(!input.contains(&"あ".repeat(SEED_SUMMARY_MAX_CHARS)));
        // 要点は最大件数までに抑える。
        let many = (0..10).map(|i| format!("要点{i}")).collect::<Vec<_>>();
        let input = build_yuuko_speech_input("t", Some("s"), &many);
        assert_eq!(
            input.lines().filter(|line| line.starts_with('・')).count(),
            KEY_POINTS_MAX_ITEMS
        );
    }

    #[test]
    fn mock_generation_succeeds_for_risky_external_text() {
        // 抜粋の HTML・型引数風の `<`・行頭ハッシュタグ・区切り行、5000文字の抜粋、
        // `<T>` を含み `#1` で始まるタイトルでも、Mock 結果は無害化された種から作られ検証を通る。
        let risky_excerpt = "本文に<script>x</script>が混入。\nVec<String> と Promise<void> の話。\n#AI #生成AI\n---\n後半";
        let long_excerpt = "あ".repeat(5_000);
        let risky_title = "#1 LazyCell<T> の使い方";
        for (name, title, excerpt) in [
            ("risky-excerpt", "半導体工場の新設計画", risky_excerpt),
            (
                "long-excerpt",
                "半導体工場の新設計画",
                long_excerpt.as_str(),
            ),
            ("risky-title", risky_title, "通常の抜粋です。"),
        ] {
            let root_dir = temp_root(name);
            let (service, repository) = build_service_with_title(&root_dir, title, excerpt);

            let generated = service.generate_article_summary(params()).unwrap();
            for (kind, text) in [
                (SummaryOutputKind::Summary, &generated.summary),
                (SummaryOutputKind::Explanation, &generated.yuuko_explanation),
                (SummaryOutputKind::Comment, &generated.yuuko_comment),
            ] {
                assert!(validate_summary_output(kind, text).is_ok());
            }

            // Markdown 再読込でもセクションが崩れず、抜粋・生成値がそのまま読める。
            let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
            assert_eq!(detail.excerpt.as_deref(), Some(excerpt));
            assert_eq!(detail.summary.as_deref(), Some(generated.summary.as_str()));
            assert_eq!(
                detail.yuuko_explanation.as_deref(),
                Some(generated.yuuko_explanation.as_str())
            );
            assert_eq!(
                detail.yuuko_comment.as_deref(),
                Some(generated.yuuko_comment.as_str())
            );
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    #[test]
    fn neutralize_seed_text_makes_text_pass_validation() {
        let neutralized = neutralize_seed_text("a<b>c\n  #AI\n---\nok\u{0007}\t末尾", usize::MAX);
        assert_eq!(neutralized, "a＜b>c\n  ＃AI\nok\t末尾");
        assert!(validate_summary_output(SummaryOutputKind::Summary, &neutralized).is_ok());
        // 文字数（chars）で切り詰める。
        assert_eq!(neutralize_seed_text(&"あ".repeat(10), 3), "あああ");
    }

    #[test]
    fn provider_is_recorded_as_mock_when_any_output_fell_back() {
        let gemini = response("a", "gemini");
        let mock = response("b", "mock");
        assert_eq!(combined_provider(&[&gemini, &gemini, &gemini]), "gemini");
        assert_eq!(combined_provider(&[&gemini, &gemini, &mock]), "mock");
        assert_eq!(combined_provider(&[&mock, &gemini, &gemini]), "mock");
    }

    #[test]
    fn invalid_generation_keeps_existing_saved_summary() {
        let root_dir = temp_root("invalid");
        let (service, repository) =
            build_service(&root_dir, "新しい半導体工場の建設計画が発表されました。");
        repository
            .update_article_summary(
                ARTICLE_ID,
                ArticleSummaryUpdate {
                    summary: "保存済みの要約".to_string(),
                    yuuko_explanation: "保存済みの再説明".to_string(),
                    key_points: Vec::new(),
                    focus_points: vec!["観点A".to_string()],
                    yuuko_comment: "保存済みの一言".to_string(),
                    tags: crate::domain::article::ArticleTagsUpdate::Keep,
                    ai_provider: "mock".to_string(),
                    generated_at: "2026-10-01T00:00:00Z".to_string(),
                },
            )
            .unwrap();

        // 要約・再説明は有効だが、感想は検証に落ちる Mock 出力を注入する。
        let error = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| match kind {
                    SummaryOutputKind::Comment => {
                        select_valid_output(kind, response("## ゆうこの一言\n混入した出力", "mock"))
                    }
                    _ => select_valid_output(kind, response("新しい出力", "mock")),
                },
            )
            .unwrap_err();
        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "PARSE_ERROR");
        assert!(!command_error.message.contains("混入した出力"));

        // 一部だけ成功していても保存せず、既存の保存値は上書きされない。
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("保存済みの要約"));
        assert_eq!(
            detail.yuuko_explanation.as_deref(),
            Some("保存済みの再説明")
        );
        assert_eq!(detail.yuuko_comment.as_deref(), Some("保存済みの一言"));
        assert_eq!(detail.focus_points, vec!["観点A".to_string()]);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    /// 実AI（gemini）の呼び出しを、指定した種別だけ失敗させて Mock へ切り替える注入関数
    /// （Gemini の通信失敗・利用枠超過で `AiProviderService` が Mock を返した状態）。
    fn gemini_with_fallback_on(
        failing: SummaryOutputKind,
    ) -> impl Fn(
        crate::domain::summary::AiRequest,
        SummaryOutputKind,
        crate::domain::settings::AiProvider,
        crate::domain::settings::ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        move |_request, kind, _provider, _level| {
            if kind == failing {
                select_valid_output(kind, output(kind, "代わりの出力", "mock"))
            } else {
                Ok(output(kind, "実AIの出力", "gemini"))
            }
        }
    }

    #[test]
    fn auto_summary_does_not_save_fallback_output() {
        let root_dir = temp_root("strict-fallback");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let result = service.generate_article_summary_with(
            params(),
            FallbackPolicy::RejectFallback,
            gemini_with_fallback_on(SummaryOutputKind::Explanation),
        );

        assert!(result.is_err());
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert!(detail.yuuko_comment.is_none());
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_saves_when_all_outputs_come_from_real_ai() {
        let root_dir = temp_root("strict-real");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::RejectFallback,
                |_request, kind, _provider, _level| Ok(output(kind, "実AIの出力", "gemini")),
            )
            .unwrap();

        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn manual_summary_with_invalid_real_ai_output_is_not_saved() {
        // D104: 手動の要約生成でも、実AI（Gemini・ローカル）の出力が検証に落ちたら Mock を保存しない。
        for provider in ["gemini", "local"] {
            let root_dir = temp_root(&format!("manual-rejected-{provider}"));
            let (service, repository) =
                build_service(&root_dir, "新しい半導体工場の建設計画です。");

            // 未要約の記事: 未要約のまま残る。
            let calls = RefCell::new(Vec::new());
            let error = service
                .generate_article_summary_with(
                    params(),
                    FallbackPolicy::SaveFallback,
                    |_request, kind, _provider, _level| {
                        calls.borrow_mut().push(kind);
                        if kind == SummaryOutputKind::Explanation {
                            return select_valid_output(
                                kind,
                                response("## 見出しの混入", provider),
                            );
                        }
                        select_valid_output(kind, output(kind, "実AIの出力", provider))
                    },
                )
                .unwrap_err();
            assert_eq!(CommandError::from(error).code, "AI_OUTPUT_REJECTED");
            // 落ちた時点で止め、残りの AI 呼び出し（感想）はしない。
            assert_eq!(
                calls.into_inner(),
                vec![
                    SummaryOutputKind::Points,
                    SummaryOutputKind::Summary,
                    SummaryOutputKind::Explanation
                ]
            );
            assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());

            // 要約済みの記事: 保存済みの要約は上書きされない。
            service
                .generate_article_summary_with(
                    params(),
                    FallbackPolicy::SaveFallback,
                    |_request, kind, _provider, _level| {
                        select_valid_output(kind, output(kind, "保存済みの出力", provider))
                    },
                )
                .unwrap();
            let error = service
                .generate_article_summary_with(
                    params(),
                    FallbackPolicy::SaveFallback,
                    |_request, kind, _provider, _level| {
                        select_valid_output(kind, response("<p>壊れた出力</p>", provider))
                    },
                )
                .unwrap_err();
            assert!(matches!(error, AppError::AiOutputRejected));
            let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
            assert_eq!(detail.summary.as_deref(), Some("保存済みの出力"));
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    #[test]
    fn manual_summary_still_saves_fallback_output() {
        let root_dir = temp_root("manual-fallback");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let generated = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                gemini_with_fallback_on(SummaryOutputKind::Explanation),
            )
            .unwrap();

        let SummaryGeneration::Generated(generated) = generated else {
            panic!("manual generation must always save");
        };
        assert_eq!(generated.yuuko_explanation, "代わりの出力");
        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    /// 呼ばれた出力種別を記録しつつ、指定した種別だけ Mock へ切り替える注入関数を使って生成する。
    fn generate_counting_calls(
        service: &SummaryService,
        policy: FallbackPolicy,
        failing: SummaryOutputKind,
    ) -> (Result<SummaryGeneration, AppError>, Vec<SummaryOutputKind>) {
        let calls = RefCell::new(Vec::new());
        let inner = gemini_with_fallback_on(failing);
        let result = service.generate_article_summary_with(
            params(),
            policy,
            |request, kind, provider, level| {
                calls.borrow_mut().push(kind);
                inner(request, kind, provider, level)
            },
        );
        (result, calls.into_inner())
    }

    #[test]
    fn auto_summary_stops_calling_ai_after_the_first_fallback() {
        let root_dir = temp_root("strict-abort");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        // 1つ目（要点）で Mock に切り替わったら、要約・再説明・感想の AI 呼び出しはしない（D167 の順）。
        let (result, calls) = generate_counting_calls(
            &service,
            FallbackPolicy::RejectFallback,
            SummaryOutputKind::Points,
        );
        assert!(result.is_err());
        assert_eq!(calls, vec![SummaryOutputKind::Points]);

        // 2つ目（要約）で切り替わったら、作り直し・再説明・感想は呼ばない。
        let (result, calls) = generate_counting_calls(
            &service,
            FallbackPolicy::RejectFallback,
            SummaryOutputKind::Summary,
        );
        assert!(result.is_err());
        assert_eq!(
            calls,
            vec![SummaryOutputKind::Points, SummaryOutputKind::Summary]
        );

        // 3つ目（再説明）で切り替わったら、感想は呼ばない。
        let (result, calls) = generate_counting_calls(
            &service,
            FallbackPolicy::RejectFallback,
            SummaryOutputKind::Explanation,
        );
        assert!(result.is_err());
        assert_eq!(
            calls,
            vec![
                SummaryOutputKind::Points,
                SummaryOutputKind::Summary,
                SummaryOutputKind::Explanation
            ]
        );
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());

        // 手動生成は従来どおり3出力とも生成する。
        let (result, calls) = generate_counting_calls(
            &service,
            FallbackPolicy::SaveFallback,
            SummaryOutputKind::Summary,
        );
        assert!(result.is_ok());
        assert_eq!(calls.len(), 4);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_does_not_overwrite_a_manual_summary_saved_during_the_ai_call() {
        let root_dir = temp_root("strict-concurrent-manual");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let result = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::RejectFallback,
                |_request, kind, _provider, _level| {
                    if kind == SummaryOutputKind::Comment {
                        // 自動要約の AI 呼び出し中に、同じ記事を手動で要約・保存した状態を再現する。
                        repository
                            .update_article_summary(
                                ARTICLE_ID,
                                ArticleSummaryUpdate {
                                    summary: "手動の要約".to_string(),
                                    yuuko_explanation: "手動の再説明".to_string(),
                                    key_points: Vec::new(),
                                    focus_points: vec!["手動の観点".to_string()],
                                    yuuko_comment: "手動の一言".to_string(),
                                    tags: crate::domain::article::ArticleTagsUpdate::Keep,
                                    ai_provider: "gemini".to_string(),
                                    generated_at: "2026-10-08T00:00:00Z".to_string(),
                                },
                            )
                            .unwrap();
                    }
                    Ok(output(kind, "自動の出力", "gemini"))
                },
            )
            .unwrap();

        // 失敗ではなく「要約済みのため保存しなかった」として返り、手動の保存値が残る。
        assert!(matches!(result, SummaryGeneration::AlreadySummarized));
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("手動の要約"));
        assert_eq!(detail.yuuko_explanation.as_deref(), Some("手動の再説明"));
        assert_eq!(detail.yuuko_comment.as_deref(), Some("手動の一言"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_stops_before_the_next_stage_when_a_manual_summary_starts() {
        let root_dir = temp_root("auto-yield-manual");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let calls = RefCell::new(Vec::new());
        let manual = RefCell::new(None);
        let outcome = service
            .generate_auto_summary_with(
                params(),
                |_| true,
                |_request, kind, _provider, _level| {
                    calls.borrow_mut().push(kind);
                    // 1段目の生成中に、利用者が同じ記事の「今すぐ要約」を押した。
                    manual.replace(Some(service.mark_manual_summary_in_progress(ARTICLE_ID)));
                    Ok(output(kind, "自動の出力", "gemini"))
                },
            )
            .unwrap();

        // 残りの段は呼ばず、保存もしない（失敗ではなく手動へ譲った扱い）。
        assert_eq!(outcome, super::AutoSummaryOutcome::YieldedToManual);
        assert_eq!(*calls.borrow(), vec![SummaryOutputKind::Points]);
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
        drop(manual);
        assert!(!service.is_manual_summary_in_progress(ARTICLE_ID));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_stops_before_the_next_stage_when_the_article_was_summarized() {
        let root_dir = temp_root("auto-yield-saved");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let calls = RefCell::new(0);
        let outcome = service
            .generate_auto_summary_with(
                params(),
                |_| true,
                |_request, kind, _provider, _level| {
                    *calls.borrow_mut() += 1;
                    // 1段目の生成中に、手動の要約が保存まで終わった。
                    repository
                        .update_article_summary(
                            ARTICLE_ID,
                            ArticleSummaryUpdate {
                                summary: "手動の要約".to_string(),
                                yuuko_explanation: "手動の再説明".to_string(),
                                key_points: Vec::new(),
                                focus_points: vec!["手動の観点".to_string()],
                                yuuko_comment: "手動の一言".to_string(),
                                tags: crate::domain::article::ArticleTagsUpdate::Keep,
                                ai_provider: "gemini".to_string(),
                                generated_at: "2026-10-08T00:00:00Z".to_string(),
                            },
                        )
                        .unwrap();
                    Ok(output(kind, "自動の出力", "gemini"))
                },
            )
            .unwrap();

        assert_eq!(outcome, super::AutoSummaryOutcome::AlreadySummarized);
        assert_eq!(*calls.borrow(), 1, "the remaining stages are not generated");
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("手動の要約"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_rechecks_after_waiting_for_manual_priority() {
        let root_dir = temp_root("auto-wait-recheck");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        // 1段目を送る前の待ち（手動の要求の優先）の間に、利用者が同じ記事を手動で要約し終えた。
        let waits = RefCell::new(0);
        let outcome = service
            .generate_auto_summary_with(
                params(),
                |_provider| {
                    *waits.borrow_mut() += 1;
                    repository
                        .update_article_summary(
                            ARTICLE_ID,
                            ArticleSummaryUpdate {
                                summary: "手動の要約".to_string(),
                                yuuko_explanation: "手動の再説明".to_string(),
                                key_points: Vec::new(),
                                focus_points: vec!["手動の観点".to_string()],
                                yuuko_comment: "手動の一言".to_string(),
                                tags: crate::domain::article::ArticleTagsUpdate::Keep,
                                ai_provider: "local".to_string(),
                                generated_at: "2026-10-08T00:00:00Z".to_string(),
                            },
                        )
                        .unwrap();
                    true
                },
                |_request, _kind, _provider, _level| {
                    panic!("must not generate a stage after the article was summarized")
                },
            )
            .unwrap();
        assert_eq!(outcome, super::AutoSummaryOutcome::AlreadySummarized);
        assert_eq!(*waits.borrow(), 1);

        // 待ちの間に手動要約が始まった（まだ生成中）場合も、送らずに手を引く。
        let root_dir2 = temp_root("auto-wait-recheck-manual");
        let (service2, _) = build_service(&root_dir2, "新しい半導体工場の建設計画です。");
        let manual = RefCell::new(None);
        let outcome = service2
            .generate_auto_summary_with(
                params(),
                |_provider| {
                    manual.replace(Some(service2.mark_manual_summary_in_progress(ARTICLE_ID)));
                    true
                },
                |_request, _kind, _provider, _level| panic!("must yield to the manual summary"),
            )
            .unwrap();
        assert_eq!(outcome, super::AutoSummaryOutcome::YieldedToManual);

        // アプリ終了で待ちが打ち切られたら、生成せずにエラーで終わる。
        drop(manual);
        let result = service2.generate_auto_summary_with(
            params(),
            |_provider| false,
            |_request, _kind, _provider, _level| panic!("must not generate while exiting"),
        );
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&root_dir);
        let _ = std::fs::remove_dir_all(&root_dir2);
    }

    #[test]
    fn auto_summary_without_manual_overlap_runs_all_stages_and_saves() {
        let root_dir = temp_root("auto-no-overlap");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let calls = RefCell::new(0);
        let outcome = service
            .generate_auto_summary_with(
                params(),
                |_| true,
                |_request, kind, _provider, _level| {
                    *calls.borrow_mut() += 1;
                    Ok(output(kind, "自動の出力", "gemini"))
                },
            )
            .unwrap();
        assert_eq!(outcome, super::AutoSummaryOutcome::Saved);
        assert_eq!(*calls.borrow(), 4);
        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn manual_summary_is_in_progress_only_while_generating() {
        let root_dir = temp_root("manual-registry");
        let (service, _) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        // 同じ記事の手動要約が重なっても、片方が終わっただけでは外さない。
        let first = service.mark_manual_summary_in_progress(ARTICLE_ID);
        let second = service.mark_manual_summary_in_progress(ARTICLE_ID);
        assert!(service.is_manual_summary_in_progress(ARTICLE_ID));
        assert!(!service.is_manual_summary_in_progress("other"));
        drop(first);
        assert!(service.is_manual_summary_in_progress(ARTICLE_ID));
        drop(second);
        assert!(!service.is_manual_summary_in_progress(ARTICLE_ID));

        // 手動の要約（既定の Mock）は、終われば成功でも外れている。
        service.generate_article_summary(params()).unwrap();
        assert!(!service.is_manual_summary_in_progress(ARTICLE_ID));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn manual_summary_still_overwrites_an_existing_summary() {
        let root_dir = temp_root("manual-overwrite");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| Ok(output(kind, "1回目の出力", "gemini")),
            )
            .unwrap();

        // 手動の再生成は要約済みでも作り直す（従来どおり）。
        let regenerated = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| Ok(output(kind, "2回目の出力", "gemini")),
            )
            .unwrap();
        assert!(matches!(regenerated, SummaryGeneration::Generated(_)));
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("2回目の出力"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn nickname_is_never_sent_to_ai() {
        // 呼び名は吹き出しの表示にだけ使い、AI へ送る入力（本文・文脈）には含めない（要件定義書 §7.6.5）。
        let root_dir = temp_root("nickname-not-sent");
        let (service, _repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        let nickname = "呼び名検査用ニックネーム";
        let mut settings = crate::domain::settings::PersistedSettings::default();
        settings.user.nickname = nickname.to_string();
        SettingsRepository::with_path(root_dir.join("settings.json"))
            .save(&settings)
            .unwrap();

        let requests = RefCell::new(Vec::new());
        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |request, kind, _provider, _level| {
                    requests.borrow_mut().push(request);
                    Ok(output(kind, "実AIの出力", "gemini"))
                },
            )
            .unwrap();

        let requests = requests.into_inner();
        assert_eq!(requests.len(), 4);
        for request in requests {
            assert!(!request.input_text.contains(nickname));
            assert!(!request.context.unwrap_or_default().contains(nickname));
        }
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    // --- 要点・注目ポイント（判断台帳 D18） ---

    fn points_text(key_points: &[&str], focus_points: &[&str]) -> String {
        serde_json::json!({ "key_points": key_points, "focus_points": focus_points }).to_string()
    }

    #[test]
    fn article_points_are_parsed_trimmed_and_kept_separate() {
        let text = points_text(
            &["  工場の新設が発表された ", "投資額は1兆円", "稼働は2028年"],
            &["地域の雇用がどう変わるかに注目"],
        );
        let points = validate_article_points(&text).unwrap();
        // tags の無い出力（旧形式・Gemini が省いた場合）も、要点・注目ポイントは受け付ける。
        assert_eq!(points.tags_rejection, Some(OutputRejection::Missing));
        let points = points.points;
        assert!(points.tags.is_empty());
        assert_eq!(
            points.key_points,
            vec!["工場の新設が発表された", "投資額は1兆円", "稼働は2028年"]
        );
        assert_eq!(points.focus_points, vec!["地域の雇用がどう変わるかに注目"]);

        // 全体を囲む1組のコードフェンス（Gemini で起きうる）だけは外して読む。
        let fenced = format!("```json\n{text}\n```");
        assert_eq!(validate_article_points(&fenced).unwrap().points, points);
        // 比較の「<」や途中のハイフンは拒否しない。
        assert!(
            validate_article_points(&points_text(&["1 < 2 の話", "A-B 間の接続"], &["x"])).is_ok()
        );
    }

    #[test]
    fn article_points_with_unexpected_shape_are_rejected() {
        for text in [
            "要点は次のとおりです".to_string(),
            format!("要点です: {}", points_text(&["a", "b"], &["c"])),
            r#"{"key_points": ["a", "b"]}"#.to_string(),
            r#"{"key_points": ["a", "b"], "focus_points": ["c"], "extra": 1}"#.to_string(),
            r#"{"key_points": "a", "focus_points": ["c"]}"#.to_string(),
            r#"["a", "b"]"#.to_string(),
        ] {
            assert_eq!(
                validate_article_points(&text),
                Err(OutputRejection::InvalidJson),
                "{text}"
            );
        }
    }

    #[test]
    fn article_points_out_of_count_range_are_rejected() {
        for (key_points, focus_points) in [
            (vec!["a"], vec!["c"]),
            (vec!["a", "b", "c", "d", "e"], vec!["c"]),
            (vec!["a", "b"], vec![]),
            (vec!["a", "b"], vec!["c", "d", "e", "f"]),
        ] {
            assert_eq!(
                validate_article_points(&points_text(&key_points, &focus_points)),
                Err(OutputRejection::ItemCount)
            );
        }
        // 下限・上限ちょうどは受け付ける。
        assert!(validate_article_points(&points_text(&["a", "b"], &["c"])).is_ok());
        assert!(
            validate_article_points(&points_text(&["a", "b", "c", "d"], &["e", "f", "g"])).is_ok()
        );
    }

    #[test]
    fn article_point_items_with_unsafe_content_are_rejected() {
        let at_limit = "あ".repeat(POINT_ITEM_MAX_CHARS);
        let over_limit = "あ".repeat(POINT_ITEM_MAX_CHARS + 1);
        assert!(validate_article_points(&points_text(&[&at_limit, "b"], &["c"])).is_ok());
        for (item, expected) in [
            (over_limit.as_str(), OutputRejection::TooLong),
            ("   ", OutputRejection::Empty),
            ("1行目\n## ゆうこの一言", OutputRejection::MultiLine),
            ("改行\r混入", OutputRejection::MultiLine),
            ("## 要点", OutputRejection::MarkdownHeading),
            ("---", OutputRejection::MarkdownSeparator),
            ("--", OutputRejection::MarkdownSeparator),
            ("<script>x</script>", OutputRejection::HtmlTag),
            ("制御\u{0007}文字", OutputRejection::ControlCharacter),
        ] {
            // 要点側・注目ポイント側のどちらに混ざっても拒否する。
            assert_eq!(
                validate_article_points(&points_text(&[item, "b"], &["c"])),
                Err(expected),
                "{item:?}"
            );
            assert_eq!(
                validate_article_points(&points_text(&["a", "b"], &[item])),
                Err(expected),
                "{item:?}"
            );
        }
        // 応答全体が 16KiB を超えるものは解析せずに拒否する。
        let huge = format!(
            "{}{}",
            " ".repeat(16 * 1024),
            points_text(&["a", "b"], &["c"])
        );
        assert_eq!(
            validate_article_points(&huge),
            Err(OutputRejection::TooLong)
        );
    }

    #[test]
    fn generated_key_points_and_focus_points_are_saved_separately_and_reload() {
        let root_dir = temp_root("points-saved");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let generated = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |request, kind, _provider, _level| {
                    if kind == SummaryOutputKind::Points {
                        // 要点は記事の情報（タイトル・抜粋）だけから作り、プロンプトIDも専用のものを使う。
                        assert_eq!(request.prompt_id, ARTICLE_POINTS_PROMPT_ID);
                        assert!(request.input_text.contains("半導体工場の新設計画"));
                        assert!(request
                            .input_text
                            .contains("新しい半導体工場の建設計画です。"));
                        return select_valid_output(
                            kind,
                            response(
                                &points_text(
                                    &["工場の新設が発表された", "投資額は1兆円", "稼働は来年度"],
                                    &["地域の雇用がどう変わるかに注目"],
                                ),
                                "local",
                            ),
                        );
                    }
                    select_valid_output(kind, response("ローカルの出力", "local"))
                },
            )
            .unwrap();
        let SummaryGeneration::Generated(generated) = generated else {
            panic!("manual generation must save");
        };
        assert_eq!(
            generated.key_points,
            vec!["工場の新設が発表された", "投資額は1兆円", "稼働は来年度"]
        );
        assert_eq!(
            generated.focus_points,
            vec!["地域の雇用がどう変わるかに注目"]
        );

        // 記事ファイルを読み直しても、要点と注目ポイントが混ざらずに戻る。
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.key_points, generated.key_points);
        assert_eq!(detail.focus_points, generated.focus_points);
        assert_eq!(detail.summary.as_deref(), Some("ローカルの出力"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_key_points_come_from_the_article_and_differ_from_focus_points() {
        let root_dir = temp_root("points-mock");
        let (service, repository) = build_service(
            &root_dir,
            "新しい半導体工場の建設計画が発表されました。投資額は1兆円です。稼働は2028年の予定です。",
        );

        // 既定（provider=Mock）でも決定的な要点・注目ポイントが作られ、検証を通って保存される。
        let generated = service.generate_article_summary(params()).unwrap();
        assert_eq!(
            generated.key_points,
            vec![
                "新しい半導体工場の建設計画が発表されました",
                "投資額は1兆円です",
                "稼働は2028年の予定です"
            ]
        );
        assert!(!generated.focus_points.is_empty());
        for point in &generated.focus_points {
            assert!(!generated.key_points.contains(point));
            assert!(!point.contains("mock"));
        }
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.key_points, generated.key_points);
        assert_eq!(detail.focus_points, generated.focus_points);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_key_points_pass_validation_for_risky_or_short_text() {
        for (name, title, excerpt) in [
            (
                "points-risky",
                "#1 LazyCell<T> の使い方",
                "前半。---。#見出し風。- 箇条書き風。<b>太字</b>",
            ),
            ("points-short", "短いタイトル", "一文だけ"),
            ("points-long", "長い記事", &"あ".repeat(5_000)),
        ] {
            let root_dir = temp_root(name);
            let (service, repository) = build_service_with_title(&root_dir, title, excerpt);
            let generated = service.generate_article_summary(params()).unwrap();
            assert!(
                (KEY_POINTS_MIN_ITEMS..=KEY_POINTS_MAX_ITEMS).contains(&generated.key_points.len()),
                "{name}"
            );
            for point in generated.key_points.iter().chain(&generated.focus_points) {
                assert!(validate_point_item(point).is_ok(), "{name}: {point}");
            }
            assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    #[test]
    fn malformed_points_output_is_not_saved_and_keeps_the_article_file_intact() {
        for policy in [FallbackPolicy::SaveFallback, FallbackPolicy::RejectFallback] {
            for provider in ["gemini", "local"] {
                let root_dir = temp_root(&format!("points-malformed-{provider}"));
                let (service, repository) =
                    build_service(&root_dir, "新しい半導体工場の建設計画です。");

                // 検証を通さずに崩れた出力を返す取得手段でも、保存前の検証で止まる。
                for malformed in [
                    "要点: 工場ができる".to_string(),
                    points_text(&["1件だけ"], &["注目"]),
                    points_text(&["a\n## ゆうこの一言", "b"], &["注目"]),
                ] {
                    let error = service
                        .generate_article_summary_with(
                            params(),
                            policy,
                            |_request, kind, _provider, _level| {
                                if kind == SummaryOutputKind::Points {
                                    Ok(response(&malformed, provider))
                                } else {
                                    Ok(response("実AIの出力", provider))
                                }
                            },
                        )
                        .unwrap_err();
                    assert!(matches!(error, AppError::AiOutputRejected));
                    assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
                }

                // 記事ファイルは壊れず、未要約のまま読み込める。
                let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
                assert!(detail.key_points.is_empty());
                assert!(detail.focus_points.is_empty());
                assert!(detail.yuuko_comment.is_none());
                let _ = std::fs::remove_dir_all(&root_dir);
            }
        }
    }

    #[test]
    fn auto_summary_does_not_save_mock_points() {
        // D56: 自動要約は、要点・注目ポイントだけが Mock に切り替わった場合も保存しない。
        let root_dir = temp_root("points-auto-fallback");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        let result = service.generate_article_summary_with(
            params(),
            FallbackPolicy::RejectFallback,
            gemini_with_fallback_on(SummaryOutputKind::Points),
        );
        assert!(result.is_err());
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    // --- 記事タグ（D11） ---

    fn points_with_tags(tags: serde_json::Value) -> String {
        serde_json::json!({
            "key_points": ["工場の新設が発表された", "投資額は1兆円"],
            "focus_points": ["地域の雇用がどう変わるかに注目"],
            "tags": tags,
        })
        .to_string()
    }

    /// 要点の出力だけを差し替え、他の出力は検証を通る実AIの出力にする取得手段で生成する。
    fn generate_with_points(
        service: &SummaryService,
        policy: FallbackPolicy,
        points: &str,
        provider: &str,
    ) -> Result<SummaryGeneration, AppError> {
        service.generate_article_summary_with(params(), policy, |_request, kind, _p, _l| {
            if kind == SummaryOutputKind::Points {
                return select_valid_output(kind, response(points, provider));
            }
            select_valid_output(kind, response(&format!("{provider}の出力"), provider))
        })
    }

    #[test]
    fn article_tags_are_trimmed_and_deduplicated() {
        let validated = validate_article_points(&points_with_tags(serde_json::json!([
            " 半導体 ",
            "AI",
            "ai",
            "半導体",
            "地域経済"
        ])))
        .unwrap();
        assert_eq!(validated.tags_rejection, None);
        assert_eq!(validated.points.tags, vec!["半導体", "AI", "地域経済"]);

        // 件数は重複を除いた後で数える（生の6件でも、除いて5件なら受け付ける）。
        let validated = validate_article_points(&points_with_tags(serde_json::json!([
            "AI", "ai", "b", "c", "d", "e"
        ])))
        .unwrap();
        assert_eq!(validated.tags_rejection, None);
        assert_eq!(validated.points.tags, vec!["AI", "b", "c", "d", "e"]);

        // 上限の文字数（20文字）と、YAML で特別な意味を持ちうる値もタグとしては受け付ける
        // （保存時は serde_yaml が引用符を付ける）。
        let at_limit = "あ".repeat(ARTICLE_TAG_MAX_CHARS);
        let validated = validate_article_points(&points_with_tags(serde_json::json!([
            at_limit, "C++", "R&D", "null", "a: b"
        ])))
        .unwrap();
        assert_eq!(validated.tags_rejection, None);
        assert_eq!(validated.points.tags.len(), 5);

        // ローカルLLMが返しがちなハッシュタグ形式（実機で6回とも `#中小企業補助金` の形だった）は、
        // 先頭の `#`・`＃` を取り除いて受け付ける。取り除いた結果の重複もまとめる。
        let validated = validate_article_points(&points_with_tags(serde_json::json!([
            "#中小企業補助金",
            "＃デジタル化",
            "## 人手不足",
            "#デジタル化",
            "＃＃量子計算",
        ])))
        .unwrap();
        assert_eq!(validated.tags_rejection, None);
        assert_eq!(
            validated.points.tags,
            vec!["中小企業補助金", "デジタル化", "人手不足", "量子計算"]
        );
        let validated =
            validate_article_points(&points_with_tags(serde_json::json!(["# #AI", "b", "c"])))
                .unwrap();
        assert_eq!(validated.points.tags, vec!["AI", "b", "c"]);
    }

    #[test]
    fn invalid_article_tags_drop_only_the_tags_and_keep_the_points() {
        let too_long = "あ".repeat(ARTICLE_TAG_MAX_CHARS + 1);
        for (tags, expected) in [
            (serde_json::json!(["a", "b"]), OutputRejection::ItemCount),
            // 重複を除いても6件。
            (
                serde_json::json!(["a", "b", "c", "d", "e", "f"]),
                OutputRejection::ItemCount,
            ),
            // 生の件数が上限の2倍（10件）を超える。
            (
                serde_json::json!(["a", "a", "a", "a", "a", "a", "a", "a", "b", "c", "d"]),
                OutputRejection::ItemCount,
            ),
            // 重複を除くと2件になる。
            (
                serde_json::json!(["AI", "ai", "半導体"]),
                OutputRejection::ItemCount,
            ),
            (
                serde_json::json!(["a", "b", too_long]),
                OutputRejection::TooLong,
            ),
            (serde_json::json!(["a", "b", "  "]), OutputRejection::Empty),
            (
                serde_json::json!(["a", "b", "c\nd"]),
                OutputRejection::MultiLine,
            ),
            (
                serde_json::json!(["a", "b", "c\td"]),
                OutputRejection::ControlCharacter,
            ),
            (
                serde_json::json!(["a", "b", "c\u{7}"]),
                OutputRejection::ControlCharacter,
            ),
            (serde_json::json!(["a", "b", "#"]), OutputRejection::Empty),
            (
                serde_json::json!(["a", "b", " ＃ "]),
                OutputRejection::Empty,
            ),
            (
                serde_json::json!(["a", "b", "<b>AI</b>"]),
                OutputRejection::HtmlTag,
            ),
            (
                serde_json::json!(["a", "b", "1 < 2"]),
                OutputRejection::HtmlTag,
            ),
            (
                serde_json::json!(["a", "b", "AI,半導体"]),
                OutputRejection::Separator,
            ),
            (
                serde_json::json!(["a", "b", "AI、半導体"]),
                OutputRejection::Separator,
            ),
            (
                serde_json::json!(["a", "b", "AI|半導体"]),
                OutputRejection::Separator,
            ),
            (
                serde_json::json!(["a", "b", 3]),
                OutputRejection::InvalidJson,
            ),
            (
                serde_json::json!("AI, 半導体, 経済"),
                OutputRejection::InvalidJson,
            ),
            (serde_json::Value::Null, OutputRejection::Missing),
        ] {
            let validated = validate_article_points(&points_with_tags(tags.clone()))
                .unwrap_or_else(|rejection| panic!("{tags}: points rejected ({rejection:?})"));
            assert_eq!(validated.tags_rejection, Some(expected), "{tags}");
            assert!(validated.points.tags.is_empty(), "{tags}");
            assert_eq!(validated.points.key_points.len(), 2, "{tags}");
            // 要約などの出力検証（select_valid_output の経路）でも、要点は拒否されない。
            assert!(validate_summary_output(
                SummaryOutputKind::Points,
                &points_with_tags(tags.clone())
            )
            .is_ok());
        }
        // 未知のキーは従来どおり、要点を含めて拒否する（tags 以外のキーは許さない）。
        let unknown = serde_json::json!({
            "key_points": ["a", "b"], "focus_points": ["c"], "tags": ["x", "y", "z"], "extra": 1
        })
        .to_string();
        assert_eq!(
            validate_article_points(&unknown),
            Err(OutputRejection::InvalidJson)
        );
    }

    #[test]
    fn generated_tags_are_saved_to_the_article_file_and_reload() {
        let root_dir = temp_root("tags-saved");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        let tags = ["半導体", "C++", "a: b", "null", "- 先頭ハイフン"];
        let generated = generate_with_points(
            &service,
            FallbackPolicy::SaveFallback,
            &points_with_tags(serde_json::json!(tags)),
            "local",
        )
        .unwrap();
        assert!(matches!(generated, SummaryGeneration::Generated(_)));
        // front matter（YAML）を読み直しても、同じ値・順番で戻る。
        assert_eq!(repository.get_article_tags(ARTICLE_ID).unwrap(), tags);
        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("localの出力"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn invalid_tags_do_not_block_saving_the_summary() {
        for policy in [FallbackPolicy::SaveFallback, FallbackPolicy::RejectFallback] {
            for provider in ["gemini", "local"] {
                let root_dir = temp_root(&format!("tags-invalid-{provider}"));
                let (service, repository) =
                    build_service(&root_dir, "新しい半導体工場の建設計画です。");
                let result = generate_with_points(
                    &service,
                    policy,
                    &points_with_tags(serde_json::json!(["<b>AI</b>", "半導体", "経済"])),
                    provider,
                )
                .unwrap();
                assert!(matches!(result, SummaryGeneration::Generated(_)));
                // 要約・要点は保存され、タグだけが保存されない（既存のタグ＝空のまま）。
                assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
                let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
                assert_eq!(
                    detail.summary.as_deref(),
                    Some(&*format!("{provider}の出力"))
                );
                assert_eq!(detail.key_points.len(), 2);
                assert!(repository.get_article_tags(ARTICLE_ID).unwrap().is_empty());
                let _ = std::fs::remove_dir_all(&root_dir);
            }
        }
    }

    #[test]
    fn regeneration_replaces_tags_only_when_the_new_tags_are_valid() {
        let root_dir = temp_root("tags-regenerate");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        let first = ["半導体", "工場", "投資"];
        generate_with_points(
            &service,
            FallbackPolicy::SaveFallback,
            &points_with_tags(serde_json::json!(first)),
            "local",
        )
        .unwrap();
        assert_eq!(repository.get_article_tags(ARTICLE_ID).unwrap(), first);

        // 作り直しでタグが不正・欠けていれば、要約は新しくなるがタグは前のまま残る。
        for invalid in [
            points_with_tags(serde_json::json!(["a", "b"])),
            points_text(&["要点A", "要点B"], &["注目A"]),
        ] {
            generate_with_points(&service, FallbackPolicy::SaveFallback, &invalid, "gemini")
                .unwrap();
            let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
            assert_eq!(detail.summary.as_deref(), Some("geminiの出力"));
            assert_eq!(repository.get_article_tags(ARTICLE_ID).unwrap(), first);
        }

        // 新しいタグが有効なら置き換える。
        let second = ["地域経済", "雇用", "製造業", "投資"];
        generate_with_points(
            &service,
            FallbackPolicy::SaveFallback,
            &points_with_tags(serde_json::json!(second)),
            "local",
        )
        .unwrap();
        assert_eq!(repository.get_article_tags(ARTICLE_ID).unwrap(), second);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    /// 初回起動時のサンプル記事（タグ付き）を置いた、既定（provider=Mock）のサービス。
    fn build_service_with_sample_articles(root_dir: &Path) -> (SummaryService, ArticleRepository) {
        let article_repository = ArticleRepository::with_paths(
            root_dir.join("news"),
            root_dir.join("article_favorites.json"),
            root_dir.join("archive"),
        );
        article_repository.seed_sample_articles_for_tests();
        let service = SummaryService::new(
            AiProviderService::new(&AppPaths::new(root_dir.join("ai"))),
            article_repository.clone(),
            SettingsRepository::with_path(root_dir.join("settings.json")),
        );
        (service, article_repository)
    }

    #[test]
    fn mock_summary_keeps_existing_tags_but_real_ai_tags_replace_them() {
        let root_dir = temp_root("tags-sample-mock");
        let (service, repository) = build_service_with_sample_articles(&root_dir);
        let sample_id = "article-001";
        let sample_params = || GenerateArticleSummaryParams {
            article_id: sample_id.to_string(),
        };
        let sample_tags = repository.get_article_tags(sample_id).unwrap();
        assert!(!sample_tags.is_empty());

        // Mock の要約（provider=Mock）は保存されるが、既存のタグは定型タグで上書きしない。
        service.generate_article_summary(sample_params()).unwrap();
        assert!(repository.is_article_summarized(sample_id).unwrap());
        assert_eq!(repository.get_article_tags(sample_id).unwrap(), sample_tags);

        // Gemini の失敗で Mock に切り替わった要点（タグ付き）も、既存のタグを上書きしない。
        service
            .generate_article_summary_with(
                sample_params(),
                FallbackPolicy::SaveFallback,
                |request, kind, _provider, _level| {
                    if kind == SummaryOutputKind::Points {
                        let mock = service
                            .ai_provider_service
                            .request_text(
                                request,
                                crate::domain::settings::AiProvider::Mock,
                                crate::domain::settings::ExplanationLevel::Normal,
                            )
                            .unwrap();
                        return select_valid_output(kind, mock);
                    }
                    select_valid_output(kind, response("geminiの出力", "gemini"))
                },
            )
            .unwrap();
        assert_eq!(repository.get_article_tags(sample_id).unwrap(), sample_tags);

        // 実AIの有効なタグは置き換える。
        service
            .generate_article_summary_with(
                sample_params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| {
                    if kind == SummaryOutputKind::Points {
                        return select_valid_output(
                            kind,
                            response(
                                &points_with_tags(serde_json::json!([
                                    "生成AI",
                                    "投資",
                                    "新興企業"
                                ])),
                                "local",
                            ),
                        );
                    }
                    select_valid_output(kind, response("localの出力", "local"))
                },
            )
            .unwrap();
        assert_eq!(
            repository.get_article_tags(sample_id).unwrap(),
            vec!["生成AI", "投資", "新興企業"]
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_generation_saves_deterministic_tags_from_the_genre() {
        let root_dir = temp_root("tags-mock");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        // 既定（provider=Mock）。タグの無い記事には、ジャンル「テクノロジー」を先頭にした3件が入る。
        service.generate_article_summary(params()).unwrap();
        assert_eq!(
            repository.get_article_tags(ARTICLE_ID).unwrap(),
            vec!["テクノロジー", "ニュース", "最新動向"]
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn article_file_without_tags_field_still_loads_and_gets_tags() {
        let root_dir = temp_root("tags-legacy");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        // tags の行が無い（旧形式の）記事ファイルにする。
        let path = find_article_file(&root_dir.join("news"));
        let original = std::fs::read_to_string(&path).unwrap();
        let legacy = original
            .lines()
            .filter(|line| !line.starts_with("tags:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(legacy, original.trim_end());
        std::fs::write(&path, legacy).unwrap();

        assert!(repository.get_article_tags(ARTICLE_ID).unwrap().is_empty());
        assert!(repository.get_article_detail(ARTICLE_ID).is_ok());
        generate_with_points(
            &service,
            FallbackPolicy::SaveFallback,
            &points_with_tags(serde_json::json!(["半導体", "工場", "投資"])),
            "local",
        )
        .unwrap();
        assert_eq!(
            repository.get_article_tags(ARTICLE_ID).unwrap(),
            vec!["半導体", "工場", "投資"]
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    fn find_article_file(dir: &Path) -> PathBuf {
        fn find(dir: &Path) -> Option<PathBuf> {
            std::fs::read_dir(dir).ok()?.find_map(|entry| {
                let path = entry.ok()?.path();
                if path.is_dir() {
                    find(&path)
                } else {
                    (path.file_name()?.to_string_lossy() == format!("{ARTICLE_ID}.md"))
                        .then_some(path)
                }
            })
        }
        find(dir).unwrap_or_else(|| panic!("article file not found under {}", dir.display()))
    }
}

/// 同梱ローカルLLMの品質評価ハーネス（`#[ignore]`・手動実行専用）。
#[cfg(test)]
mod local_llm_quality_eval;

/// 評価ハーネス専用のモデル・llama-server の差し替え（D168 のモデル比較。テストビルド専用）。
#[cfg(test)]
mod eval_model_override;

/// 海外（英語）記事の日本語要約の試作評価ハーネス（D163・D164。`#[ignore]`・手動実行専用）。
#[cfg(test)]
mod overseas_summary_eval;

/// 要約の機械点検（D166。数字・簡体字・指示の写し・注記・長さ・日本語らしさ）。国内記事の要約の作り直しに使い、
/// 海外記事の試作の点検（下の overseas_output_check）とも共通。
mod output_check;

/// 要約一式の見出しの残り・反対の意味・年の組み合わせ（D170）・推測の言い切り（D175）の点検。要点・要約・再説明・一言に使う。
mod consistency_check;

/// 要約・再説明・一言の文頭のつなぎの語（「これにより、」「その結果、」）を消す後処理（Firestore タスク I9iRpb）。
mod leading_connector;

/// 要約の機械点検と作り直しの評価ハーネス（D166。`#[ignore]`・手動実行専用）。
#[cfg(test)]
mod summary_output_check_eval;

/// 海外記事の要約・訳の機械点検（D165 の試作。英語の要旨の数字の読み取り。テストビルド専用）。
#[cfg(test)]
mod overseas_output_check;

/// 海外記事の多段の要約（抽出 → 用語の訳 → 組み立て → 点検）の試作評価ハーネス（D165。`#[ignore]`・手動実行専用）。
#[cfg(test)]
mod overseas_multistep_eval;
