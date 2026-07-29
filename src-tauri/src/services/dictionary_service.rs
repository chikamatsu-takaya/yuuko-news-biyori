use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::domain::dictionary::{
    normalize_text, DeleteDictionaryEntryParams, DictionaryEntryDto, DictionaryEntryListItemDto,
    DictionaryEntryType, ExplainSelectedTermParams, ListDictionaryEntriesParams,
    SaveDictionaryEntryParams, UpdateDictionaryFavoriteParams, UpdateDictionaryMemoParams,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::dictionary_repository::DictionaryRepository;

#[derive(Debug, Clone)]
pub struct DictionaryService {
    repository: DictionaryRepository,
    // 記事の存在確認に使う（実ニュース記事IDも既存取得経路で解決する）。
    article_repository: ArticleRepository,
}

impl DictionaryService {
    pub fn new(repository: DictionaryRepository, article_repository: ArticleRepository) -> Self {
        Self {
            repository,
            article_repository,
        }
    }

    /// 選択語の解説を返す。処理順は「入力検証 → 記事タイトル解決 → 辞書検索 → 判定 → DTO化」。
    /// 記事タイトルは通常 ArticleRepository で解決する。固定サンプルID(article-001〜003)は、
    /// アーカイブ保守で active Markdown が退避され NotFound になっても、組み込みタイトルへ
    /// フォールバックして固定サンプル解説へ到達できるようにする（他の未知IDは NotFound のまま）。
    ///
    /// 辞書検索は Repository が `Some`（命中）/`None`（未命中）/`Err`（検索失敗）で返し、
    /// Service が命中・未命中を判定する。未命中（`None`）分岐は、後続PRで AI Provider 呼び出しへ
    /// 置き換えられる位置。本PRでは AI を呼ばず、副作用のない純粋 helper（固定サンプル/汎用文）で返す。
    pub fn explain_selected_term(
        &self,
        params: ExplainSelectedTermParams,
    ) -> Result<DictionaryEntryDto, AppError> {
        let (article_id, selected_text) = params.validated_inputs()?;
        // 記事タイトルの解決（汎用文フォールバック用）。NotFound かつ固定サンプルIDのみ組み込み値へ。
        let article_title = self.resolve_article_title(&article_id)?;

        let normalized_text = normalize_text(&selected_text);
        // 保存済み辞書の完全一致（記事優先→記事横断の決定的選択）。固定サンプル解説より優先。命中なら即返す。
        if let Some(saved_entry) = self
            .repository
            .find_saved_entry(&article_id, &normalized_text)?
        {
            return Ok(saved_entry);
        }

        // 未命中: 後続PRではここを AI Provider 呼び出しへ置き換える。
        // 現状は固定サンプル解説 or 汎用文（AI 不使用・副作用なし）。
        Ok(build_dictionary_miss_entry(
            &article_id,
            &article_title,
            &selected_text,
        ))
    }

    /// 記事タイトルを解決する。通常は ArticleRepository。ArticleRepository が NotFound を返した
    /// 場合だけ、既知の固定サンプルID(article-001〜003)なら組み込みタイトルへフォールバックする。
    /// 固定サンプル以外の未知IDは NotFound のまま。NotFound 以外（I/O・JSON 等）は伝播する。
    /// 記事存在確認は Service の責務に留め、DictionaryRepository へは戻さない。
    fn resolve_article_title(&self, article_id: &str) -> Result<String, AppError> {
        match self.article_repository.get_article_detail(article_id) {
            Ok(article) => Ok(article.title),
            Err(AppError::NotFound(message)) => match sample_article_title(article_id) {
                Some(title) => Ok(title.to_string()),
                None => Err(AppError::NotFound(message)),
            },
            Err(other) => Err(other),
        }
    }

    pub fn list_dictionary_entries(
        &self,
        params: ListDictionaryEntriesParams,
    ) -> Result<Vec<DictionaryEntryListItemDto>, AppError> {
        let (keyword, entry_type, starred_only) = params.validated_filters()?;
        self.repository
            .list_dictionary_entries(keyword.as_deref(), entry_type, starred_only)
    }

    pub fn save_dictionary_entry(
        &self,
        params: SaveDictionaryEntryParams,
    ) -> Result<DictionaryEntryDto, AppError> {
        let entry = params.validated_entry()?;
        self.repository.save_dictionary_entry(entry)
    }

    pub fn update_dictionary_memo(
        &self,
        params: UpdateDictionaryMemoParams,
    ) -> Result<DictionaryEntryListItemDto, AppError> {
        let (entry_id, memo) = params.validated()?;
        self.repository.update_dictionary_memo(&entry_id, memo)
    }

    pub fn update_dictionary_favorite(
        &self,
        params: UpdateDictionaryFavoriteParams,
    ) -> Result<DictionaryEntryListItemDto, AppError> {
        let (entry_id, is_starred) = params.validated()?;
        self.repository
            .update_dictionary_favorite(&entry_id, is_starred)
    }

    pub fn delete_dictionary_entry(
        &self,
        params: DeleteDictionaryEntryParams,
    ) -> Result<String, AppError> {
        let entry_id = params.validated_entry_id()?;
        self.repository.delete_dictionary_entry(&entry_id)
    }
}

// --- 辞書未命中時のフォールバック（固定サンプル解説・汎用文）: 副作用なしの純粋 helper ---
// AI Provider を呼ばず、辞書ストアにも触れない。後続PRでは explain_selected_term の未命中分岐を
// AI 呼び出しへ置き換えるため、生成ロジックを Repository ではなく Service 側に集約する。

/// 既知の固定サンプル記事ID(article-001〜003)に対する組み込みタイトルを返す（純粋関数・副作用なし）。
/// 未知IDは None。active Markdown がアーカイブ退避で無くても固定サンプル解説へ到達するための
/// フォールバックにだけ使う。タイトルの二重定義を避けるため、サンプル定義から引く。
fn sample_article_title(article_id: &str) -> Option<&'static str> {
    sample_dictionary_entries()
        .into_iter()
        .find(|entry| entry.article_id == article_id)
        .map(|entry| entry.article_title)
}

/// 固定サンプル記事の解説に一致すればそれを、無ければ汎用文を返す（純粋関数）。
fn build_dictionary_miss_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
) -> DictionaryEntryDto {
    let normalized_text = normalize_text(selected_text);
    sample_dictionary_entries()
        .into_iter()
        .find(|entry| {
            entry.article_id == article_id && normalize_text(entry.key_text) == normalized_text
        })
        .map(|entry| entry.to_dto())
        .unwrap_or_else(|| build_generic_entry(article_id, article_title, selected_text))
}

#[derive(Debug, Clone)]
struct SampleDictionaryEntry {
    article_id: &'static str,
    article_title: &'static str,
    key_text: &'static str,
    entry_type: DictionaryEntryType,
    short_explanation: &'static str,
    detail_explanation: &'static str,
}

impl SampleDictionaryEntry {
    fn to_dto(&self) -> DictionaryEntryDto {
        DictionaryEntryDto {
            entry_id: build_entry_id(self.article_id, self.key_text),
            key_text: self.key_text.to_string(),
            entry_type: self.entry_type.clone(),
            short_explanation: self.short_explanation.to_string(),
            detail_explanation: self.detail_explanation.to_string(),
            related_article_id: Some(self.article_id.to_string()),
            related_article_title: Some(self.article_title.to_string()),
            is_starred: false,
        }
    }
}

fn build_entry_id(article_id: &str, selected_text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    article_id.hash(&mut hasher);
    normalize_text(selected_text).hash(&mut hasher);
    format!("entry-{}-{:x}", article_id, hasher.finish())
}

fn build_generic_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
) -> DictionaryEntryDto {
    DictionaryEntryDto {
        entry_id: build_entry_id(article_id, selected_text),
        key_text: selected_text.to_string(),
        entry_type: DictionaryEntryType::Term,
        short_explanation: format!(
            "「{selected_text}」はこの記事を理解するための補助キーワードです。"
        ),
        detail_explanation: format!(
            "「{selected_text}」は記事「{article_title}」の文脈で重要な用語です。現時点では記事理解のヒントになる簡易解説として返しています。"
        ),
        related_article_id: Some(article_id.to_string()),
        related_article_title: Some(article_title.to_string()),
        is_starred: false,
    }
}

fn sample_dictionary_entries() -> Vec<SampleDictionaryEntry> {
    vec![
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "生成AI",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "文章や画像などを自動生成する AI 全般を指す言葉です。",
            detail_explanation: "生成AIは、入力された指示に応じて文章・画像・音声などを自動生成する技術群です。この記事では、生成AIそのものの新規性よりも、業務課題の解決にどう結びついているかが注目点になっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "資金調達",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "企業が事業拡大のために投資や融資で資金を集めることです。",
            detail_explanation: "資金調達は、企業が新しい開発や採用、営業活動を進めるために必要なお金を外部から集めることです。この記事では、生成AI関連企業に再び投資が集まり始めている流れを示しています。",
        },
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "業務自動化",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "定型業務を仕組み化して人手を減らす考え方です。",
            detail_explanation: "業務自動化は、繰り返し作業や定型処理をシステム化して効率を上げる取り組みです。生成AIが評価されやすい背景には、この自動化効果を現場で示しやすいことがあります。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "SaaS",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "インターネット経由で利用するソフトウェア提供形態です。",
            detail_explanation: "SaaS は Software as a Service の略で、クラウド上で提供されるソフトウェアを必要なときに利用する形態です。この記事では、機能そのものに加えて導入後の支援体制が差別化要因として扱われています。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "導入支援",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "サービスを使い始める際の設定や定着を支える取り組みです。",
            detail_explanation: "導入支援は、ツールの初期設定だけでなく、使い方の教育や運用への定着まで含めて支援することです。この記事では、導入後の継続活用まで見据えた支援が価値として語られています。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "業務改善",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "仕事の流れを見直して効率や成果を高めることです。",
            detail_explanation: "業務改善は、現場の手間や無駄を減らしながら成果を上げるための取り組みです。この記事では、単なるツール導入ではなく、改善が定着する運用設計までが主題になっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "量子コンピュータ",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "量子力学の性質を利用して計算する新しい計算機です。",
            detail_explanation: "量子コンピュータは、通常のコンピュータとは異なる量子の性質を使って計算する技術です。この記事では高速化よりも、安定して正確に動かすための仕組みに焦点が当たっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "誤り訂正",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "計算中の誤差を検知し、補正するための仕組みです。",
            detail_explanation: "誤り訂正は、計算途中で起こるノイズや誤差を見つけて結果を安定させる考え方です。量子コンピュータでは特に重要で、実用化に向けた大きな課題の一つです。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "研究成果",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "研究や実験によって得られた新しい知見や結果です。",
            detail_explanation: "研究成果は、学術研究や実験を通じて得られた知見のことです。この記事では、新たな誤り訂正手法が今後の実装方式に影響を与える可能性がある点が重要です。",
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::article::{ArticleReadState, FetchedArticle};
    use crate::domain::dictionary::{
        DictionaryEntryDto, DictionaryEntryType, ExplainSelectedTermParams,
        SaveDictionaryEntryParams,
    };
    use crate::error::CommandError;
    use crate::repositories::article_repository::ArticleRepository;
    use crate::repositories::dictionary_repository::DictionaryRepository;

    use super::DictionaryService;

    const REAL_ARTICLE_ID: &str = "rss-20260728-tech-42";
    const REAL_ARTICLE_TITLE: &str = "実ニュース記事のタイトル";
    const OTHER_ARTICLE_ID: &str = "rss-20260728-biz-99";

    struct TestContext {
        service: DictionaryService,
        root_dir: PathBuf,
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root_dir);
        }
    }

    // 固定サンプル(article-001 等)ではない、実ニュース取得形式の記事を用意する。
    fn real_news_article(article_id: &str, title: &str) -> FetchedArticle {
        FetchedArticle {
            article_id: article_id.to_string(),
            title: title.to_string(),
            source_name: "Example News".to_string(),
            original_url: format!("https://example.com/news/{article_id}"),
            fetched_at: "2026-07-28T00:00:00Z".to_string(),
            published_at_text: "2026-07-28T00:00:00Z".to_string(),
            genre: "テクノロジー".to_string(),
            tags: Vec::new(),
            excerpt: Some("本文抜粋".to_string()),
            recommendation_score: 0.5,
            read_state: ArticleReadState::Unread,
        }
    }

    // 実ニュース記事（REAL_ARTICLE_ID）を保存元にした辞書エントリ（記事横断再利用の検証用）。
    fn saved_entry() -> DictionaryEntryDto {
        DictionaryEntryDto {
            entry_id: "entry-real-generated-ai".to_string(),
            key_text: "生成AI".to_string(),
            entry_type: DictionaryEntryType::Term,
            short_explanation: "保存済みの短い説明".to_string(),
            detail_explanation: "保存済みの詳しい説明".to_string(),
            related_article_id: Some(REAL_ARTICLE_ID.to_string()),
            related_article_title: Some(REAL_ARTICLE_TITLE.to_string()),
            is_starred: true,
        }
    }

    // テスト用に、実ニュース記事を1件だけ持つ記事リポジトリと空の辞書リポジトリで Service を組む。
    fn build_service() -> TestContext {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_dir = std::env::temp_dir().join(format!(
            "yuuko-dictionary-service-tests-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();

        let dictionary_repository =
            DictionaryRepository::with_path(root_dir.join("dictionary").join("entries.json"));
        let article_repository = ArticleRepository::with_paths(
            root_dir.join("news"),
            root_dir.join("article_favorites.json"),
            root_dir.join("archive"),
        );
        article_repository
            .save_fetched_articles(vec![
                real_news_article(REAL_ARTICLE_ID, REAL_ARTICLE_TITLE),
                real_news_article(OTHER_ARTICLE_ID, "別の実ニュース記事のタイトル"),
            ])
            .unwrap();

        let service = DictionaryService::new(dictionary_repository, article_repository);
        TestContext { service, root_dir }
    }

    fn explain(context: &TestContext, article_id: &str, selected_text: &str) -> DictionaryEntryDto {
        context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: article_id.to_string(),
                selected_text: selected_text.to_string(),
            })
            .unwrap()
    }

    #[test]
    fn explain_resolves_real_news_article_id_via_article_repository() {
        let context = build_service();
        // 前後空白は正規化方針どおり trim され、汎用文には実記事タイトルが使われる。
        let entry = explain(&context, REAL_ARTICLE_ID, "  新しい概念  ");

        assert_eq!(entry.key_text, "新しい概念");
        assert_eq!(entry.related_article_id.as_deref(), Some(REAL_ARTICLE_ID));
        assert!(entry.detail_explanation.contains(REAL_ARTICLE_TITLE));
    }

    #[test]
    fn explain_rejects_unknown_article_as_safe_not_found() {
        let context = build_service();
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "does-not-exist".to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "NOT_FOUND_ERROR");
    }

    #[test]
    fn explain_reuses_saved_explanation_for_same_and_other_articles() {
        let context = build_service();
        // 保存元は REAL_ARTICLE_ID。同じ記事でも別の実ニュース記事でも、同じ正規化語なら再利用される。
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();

        // 保存元と同じ記事で再利用（前後空白も正規化方針どおり一致）。
        let same = explain(&context, REAL_ARTICLE_ID, "  生成AI  ");
        assert_eq!(same.short_explanation, "保存済みの短い説明");
        assert!(same.is_starred);

        // 別の実ニュース記事でも同じ正規化用語なら再利用（記事横断）。
        let other = explain(&context, OTHER_ARTICLE_ID, "生成ai");
        assert_eq!(other.short_explanation, "保存済みの短い説明");
        assert!(other.is_starred);
    }

    #[test]
    fn explain_miss_does_not_persist_dictionary() {
        let context = build_service();
        let dictionary_path = context.root_dir.join("dictionary").join("entries.json");

        explain(&context, REAL_ARTICLE_ID, "未保存の用語");

        assert!(
            !dictionary_path.exists(),
            "解説表示だけで辞書ストアを作成・更新してはならない"
        );
    }

    // build_service は固定サンプル記事(article-001 等)の Markdown を seed しないため、
    // ArticleRepository.get_article_detail("article-001") は NotFound になる。
    // ＝アーカイブ退避などで active Markdown が無い状態を再現している。

    #[test]
    fn explain_falls_back_to_fixed_sample_when_sample_markdown_missing() {
        let context = build_service();

        // 固定サンプルID。ArticleRepository は NotFound だが、組み込みタイトルへフォールバックして
        // 固定サンプル解説へ到達できる。
        let entry = explain(&context, "article-001", "生成AI");
        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
        assert!(entry.short_explanation.contains("自動生成"));

        // 固定サンプルフォールバックでも辞書は保存・更新しない。
        assert!(
            !context
                .root_dir
                .join("dictionary")
                .join("entries.json")
                .exists(),
            "固定サンプルフォールバックで辞書ストアを作成してはならない"
        );
    }

    #[test]
    fn explain_prefers_saved_dictionary_over_fixed_sample_when_markdown_missing() {
        let context = build_service();
        // 保存済み辞書（"生成AI"）を用意。source は REAL_ARTICLE_ID（記事横断で命中させる）。
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();

        // article-001 の Markdown は無いが、保存済み辞書を固定サンプル解説より優先して返す。
        let entry = explain(&context, "article-001", "生成AI");
        assert_eq!(entry.short_explanation, "保存済みの短い説明");
        assert!(entry.is_starred);
    }

    #[test]
    fn explain_returns_not_found_for_unknown_non_sample_article() {
        let context = build_service();
        // 固定サンプル以外の未知IDは、フォールバックせず NotFound のまま。
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "unknown-article-xyz".to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "NOT_FOUND_ERROR");
    }

    #[test]
    fn explain_does_not_fall_back_to_sample_on_non_not_found_error() {
        let context = build_service();
        // ArticleRepository が NotFound 以外（I/O エラー）を返す状態を作る:
        // お気に入りストアの位置をディレクトリにして read_to_string を失敗させる。
        std::fs::create_dir_all(context.root_dir.join("article_favorites.json")).unwrap();

        // 固定サンプルID でも、NotFound 以外のエラーはフォールバックせずそのまま伝播する。
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "article-001".to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "IO_ERROR");
    }

    // --- 辞書未命中フォールバック（純粋 helper build_dictionary_miss_entry）の直接テスト ---

    #[test]
    fn miss_entry_returns_fixed_sample_explanation() {
        // 固定サンプル記事の解説はそのまま返す（既存挙動の維持）。
        let entry = super::build_dictionary_miss_entry(
            "article-001",
            "生成AIスタートアップの資金調達が再加速",
            "生成AI",
        );

        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
        assert!(!entry.is_starred);
        assert!(entry.short_explanation.contains("自動生成"));
    }

    #[test]
    fn miss_entry_matches_sample_case_insensitively() {
        let entry = super::build_dictionary_miss_entry(
            "article-002",
            "国内SaaS企業、業務改善支援の新施策を発表",
            "saas",
        );

        assert_eq!(entry.key_text, "SaaS");
    }

    #[test]
    fn miss_entry_returns_generic_for_unknown_term_with_real_title() {
        // 固定サンプルに無い用語は、Serviceが渡した実記事タイトルを使った汎用文を返す。
        let entry = super::build_dictionary_miss_entry(
            "rss-20260728-tech-42",
            "実ニュース記事のタイトル",
            "新しい概念",
        );

        assert_eq!(entry.key_text, "新しい概念");
        assert_eq!(
            entry.related_article_id.as_deref(),
            Some("rss-20260728-tech-42")
        );
        assert!(entry
            .detail_explanation
            .contains("実ニュース記事のタイトル"));
    }
}
