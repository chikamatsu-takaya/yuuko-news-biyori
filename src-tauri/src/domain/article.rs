use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// アーカイブ退避候補とみなす経過日数（約1か月）。データ設計書 §14.2「1か月以上前」。
/// ここでは「退避候補かどうか」の判定のみを担い、月次ZIPの実圧縮は後続で実装する。
pub const ARCHIVE_AGE_DAYS: i64 = 30;

/// 記事がアーカイブ退避候補かを判定する（純粋関数・I/Oなし）。
/// 条件（データ設計書 §14.2/§14.3）: お気に入りでない・アーカイブ済みでない・取得から約1か月以上経過。
/// `fetched_at` がRFC3339として解釈できない場合は安全側に倒し、候補にしない。
pub fn is_archive_candidate(
    fetched_at: &str,
    favorite: bool,
    is_archived: bool,
    now: DateTime<Utc>,
) -> bool {
    if favorite || is_archived {
        return false;
    }
    match parse_rfc3339_utc(fetched_at) {
        Some(fetched) => fetched <= now - Duration::days(ARCHIVE_AGE_DAYS),
        None => false,
    }
}

/// 同一出典・同一タイトルの重複判定で比べる既存記事の期間（日数）。D58（2026-10-07）。
/// 毎日同じタイトルで配信される記事（定例コラム等）を取りこぼさないよう、タイトル側の判定は
/// 直近この日数以内に取得（取得日時がなければ公開日時）された既存記事とだけ比べる。
/// URL（article_id）側の判定には期間を設けない。
pub const TITLE_DEDUPE_WINDOW_DAYS: i64 = 7;

/// 既存記事を「同一出典・同一タイトル」の比較対象に含めるか（純粋関数・I/Oなし）。
///
/// 基準時刻は `fetched_at`、解釈できなければ `published_at` を使う。どちらもRFC3339として
/// 解釈できない場合は比較対象に含めない（D58 の目的は同一タイトル記事の取りこぼし防止であり、
/// 時期不明の記事で新しい記事を落とさないため。同一URLの判定は別途期間なしで行われる）。
/// 基準時刻が `now` より未来（時計ずれ等）の場合は直近とみなして含める。
pub fn is_within_title_dedupe_window(
    fetched_at: &str,
    published_at: &str,
    now: DateTime<Utc>,
) -> bool {
    match parse_rfc3339_utc(fetched_at).or_else(|| parse_rfc3339_utc(published_at)) {
        Some(at) => at >= now - Duration::days(TITLE_DEDUPE_WINDOW_DAYS),
        None => false,
    }
}

/// 取得時の重複判定に使う既存記事のキー集合（要件定義書 §7.2.4・純粋データ・I/Oなし）。
///
/// - 同一 article_id（正規化URL由来）は重複。
/// - URLが異なっても「同一出典名かつ正規化タイトルが同一」なら重複。出典が異なれば別記事として扱う。
///
/// 1回の取得（refresh）につき1度だけ構築し、保存予定に加えた記事も `insert` で追記して、
/// 同じ取得内の重複も弾く。タイトル類似度による判定は扱わない。
/// 既存記事のタイトル側キーは `TITLE_DEDUPE_WINDOW_DAYS` 以内の記事だけを入れる（呼び出し側で絞る）。
#[derive(Debug, Clone, Default)]
pub struct ArticleDedupeKeys {
    article_ids: HashSet<String>,
    source_titles: HashSet<(String, String)>,
}

impl ArticleDedupeKeys {
    pub fn insert_id(&mut self, article_id: String) {
        self.article_ids.insert(article_id);
    }

    pub fn insert_source_title(&mut self, source_name: &str, title: &str) {
        if let Some(key) = source_title_key(source_name, title) {
            self.source_titles.insert(key);
        }
    }

    pub fn insert(&mut self, article_id: String, source_name: &str, title: &str) {
        self.insert_id(article_id);
        self.insert_source_title(source_name, title);
    }

    /// 既存記事（または同じ取得で保存予定の記事）と重複するか。
    pub fn is_duplicate(&self, article_id: &str, source_name: &str, title: &str) -> bool {
        self.article_ids.contains(article_id)
            || source_title_key(source_name, title)
                .is_some_and(|key| self.source_titles.contains(&key))
    }

    pub fn into_article_ids(self) -> HashSet<String> {
        self.article_ids
    }
}

/// 出典名×正規化タイトルのキー。タイトルが空白だけなら同一視の根拠にならないため None。
fn source_title_key(source_name: &str, title: &str) -> Option<(String, String)> {
    let normalized = normalize_title_for_dedupe(title);
    if normalized.is_empty() {
        return None;
    }
    Some((source_name.to_string(), normalized))
}

/// 重複判定用にタイトルを正規化する。
/// 前後空白の除去と、連続空白（全角空白 U+3000・タブ・改行を含む）の半角空白1つへの集約だけを行う。
/// 大文字小文字や全半角英数の統一はしない（要件にない同一視で別記事を落とさないため）。
pub fn normalize_title_for_dedupe(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// RFC3339文字列を UTC の `DateTime` へ変換する。解釈不能なら `None`。
fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|datetime| datetime.with_timezone(&Utc))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleReadState {
    Unread,
    Previewed,
    DetailViewed,
}

impl ArticleReadState {
    /// 閲覧の進み具合（Unread < Previewed < DetailViewed）。詳細設計書 §11.2 の一方向遷移を表す。
    fn progress_rank(&self) -> u8 {
        match self {
            Self::Unread => 0,
            Self::Previewed => 1,
            Self::DetailViewed => 2,
        }
    }

    /// `target` が現在より進んだ状態のときだけ true。
    /// 詳細閲覧後に軽量プレビューを開いても DetailViewed→Previewed へ後退させないために使う。
    pub fn can_advance_to(&self, target: &ArticleReadState) -> bool {
        target.progress_rank() > self.progress_rank()
    }
}

/// 記事ごとの自動要約の状態（画面表示用）。
///
/// - `Done` は記事ファイルの `status.summarized` から決まる（保存済み＝完了）。
/// - `Waiting` / `Processing` / `Failed` は自動要約キューのメモリ上の状態で、アプリ再起動で消える。
///   再試行待ちの記事は `Waiting`、再試行上限に達した記事は `Failed`。
/// - `None` は未要約でキューにも入っていない記事（自動要約が無効・まだ取得後の投入前など）。
///   キューに入っていない記事を `Waiting` と見せると、自動要約が無効なのに待っているように誤解させるため分ける。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SummaryState {
    #[default]
    None,
    Waiting,
    Processing,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleSummaryDto {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub published_at_text: String,
    pub genre: String,
    pub summary: Option<String>,
    pub is_favorite: bool,
    pub read_state: ArticleReadState,
    pub recommendation_score: f32,
    /// 自動要約の状態。ゆうこ状態ファイルに保存された旧データにも無いため既定値で読む。
    #[serde(default)]
    pub summary_state: SummaryState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleHistoryItemDto {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub published_at_text: String,
    pub fetched_at: String,
    pub genre: String,
    pub summary: Option<String>,
    pub is_favorite: bool,
    pub read_state: ArticleReadState,
    pub is_archived: bool,
    pub recommendation_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleDetailDto {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub original_url: String,
    pub published_at_text: String,
    pub genre: String,
    pub summary: Option<String>,
    /// 元の本文抜粋。AI要約の種・再生成の基にする（表示は summary を優先）。
    pub excerpt: Option<String>,
    pub yuuko_explanation: Option<String>,
    pub focus_points: Vec<String>,
    pub yuuko_comment: Option<String>,
    pub is_favorite: bool,
    pub keyword_candidates: Vec<String>,
    /// 自動要約の状態（ArticleSummaryDto と同じ意味）。
    #[serde(default)]
    pub summary_state: SummaryState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteUpdateResult {
    pub article_id: String,
    pub is_favorite: bool,
}

/// 月次アーカイブ実行の結果サマリー（command 返却用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSummaryDto {
    pub archived_article_count: usize,
    pub zip_files: Vec<ArchiveZipInfoDto>,
}

/// 1つの月次ZIPの情報（データ設計書 §14.4 のインデックス項目に対応）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveZipInfoDto {
    pub month: String,
    pub file: String,
    pub article_count: usize,
    pub size_bytes: u64,
}

/// 過去ニュース画面の月別アーカイブ一覧の1行（`archive_index.json` の月エントリから作る）。
/// ZIPファイル名・サイズなどの保存場所の情報は画面に不要なため返さない。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveMonthDto {
    /// `YYYY-MM` 形式。表示用の「2026年9月」への整形は画面側で行う。
    pub month: String,
    pub article_count: usize,
    /// false は記事カタログ未移行（v1）の月。記事一覧は返せないが件数は表示できる。
    pub catalog_complete: bool,
}

/// 指定月のアーカイブ記事一覧。ZIPは開かず、記事カタログのスナップショットを返す。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveMonthArticlesDto {
    pub month: String,
    pub catalog_complete: bool,
    pub articles: Vec<ArticleHistoryItemDto>,
}

/// アーカイブの年月（`YYYY-MM`・月は01〜12）かを判定する。
/// 年月はアーカイブファイル名の元になるため、区切り文字や相対パスを含む値を通さない。
pub fn is_valid_archive_month(month: &str) -> bool {
    let bytes = month.as_bytes();
    bytes.len() == 7
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && month
            .get(5..7)
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=12).contains(&value))
}

/// ZIPとindexの整合確認後に、通常ニュース領域から退避したMarkdownの結果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRetirementSummaryDto {
    pub retired_article_count: usize,
    pub retired_months: Vec<String>,
    /// commit後の一時退避領域を削除できなかった場合だけtrue。記事の退避自体は完了している。
    pub cleanup_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveRestoreStatus {
    Restored,
    AlreadyAvailable,
}

/// アーカイブ記事の単記事復元結果。復元先パスはセキュリティ上公開しない。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreArchivedArticleResult {
    pub article_id: String,
    pub status: ArchiveRestoreStatus,
}

/// 取得パイプライン（NewsService）が新規記事を保存する際の入力。
/// `PersistedArticleRecord` は repository 内部型のため、保存用の公開入力として用意する。
/// この型は内部Rust APIでのみ使用し、Tauri command では公開しない。
#[derive(Debug, Clone)]
pub struct FetchedArticle {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub original_url: String,
    /// 取得時刻（UTC・RFC3339）。
    pub fetched_at: String,
    pub published_at_text: String,
    pub genre: String,
    pub tags: Vec<String>,
    pub excerpt: Option<String>,
    pub recommendation_score: f32,
    pub read_state: ArticleReadState,
}

/// 生成済み要約を記事Markdownへ永続化する際の入力（内部Rust API・Tauri commandでは公開しない）。
/// B-4決定: 生成要約は記事Markdownへ保存し、再表示はキャッシュ／更新は明示再生成とする。
#[derive(Debug, Clone)]
pub struct ArticleSummaryUpdate {
    pub summary: String,
    pub yuuko_explanation: String,
    pub focus_points: Vec<String>,
    pub yuuko_comment: String,
    /// 実際に生成に使ったプロバイダ（"gemini" / "mock"）。
    pub ai_provider: String,
    /// 生成時刻（UTC・RFC3339）。
    pub generated_at: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetRecommendedArticlesParams {
    pub limit: Option<u32>,
}

impl GetRecommendedArticlesParams {
    pub fn normalized_limit(&self) -> Result<usize, AppError> {
        let limit = self.limit.unwrap_or(20);
        if limit == 0 || limit > 50 {
            return Err(AppError::Validation(
                "limit must be between 1 and 50".to_string(),
            ));
        }
        Ok(limit as usize)
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleHistoryFilter {
    #[default]
    All,
    Unread,
    Read,
    Favorite,
    Archived,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListArticleHistoryParams {
    pub limit: Option<u32>,
    pub filter: Option<ArticleHistoryFilter>,
}

impl ListArticleHistoryParams {
    pub fn normalized_limit(&self) -> Result<usize, AppError> {
        let limit = self.limit.unwrap_or(100);
        if limit == 0 || limit > 200 {
            return Err(AppError::Validation(
                "limit must be between 1 and 200".to_string(),
            ));
        }
        Ok(limit as usize)
    }

    pub fn normalized_filter(&self) -> ArticleHistoryFilter {
        self.filter.clone().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetArticleDetailParams {
    pub article_id: String,
}

impl GetArticleDetailParams {
    pub fn validated_article_id(&self) -> Result<String, AppError> {
        let article_id = self.article_id.trim();
        if article_id.is_empty() {
            return Err(AppError::Validation(
                "articleId must not be empty".to_string(),
            ));
        }

        Ok(article_id.to_string())
    }
}

/// 「元記事を開く」の入力。URL は受け取らず、記事IDから保存済みの元記事 URL を Rust 側で引く（判断台帳 D13）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOriginalArticleParams {
    pub article_id: String,
}

impl OpenOriginalArticleParams {
    pub fn validated_article_id(&self) -> Result<String, AppError> {
        let article_id = self.article_id.trim();
        if article_id.is_empty() {
            return Err(AppError::Validation(
                "articleId must not be empty".to_string(),
            ));
        }

        Ok(article_id.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateArticleFavoriteParams {
    pub article_id: String,
    pub is_favorite: bool,
}

impl UpdateArticleFavoriteParams {
    pub fn validated_inputs(&self) -> Result<(String, bool), AppError> {
        let article_id = self.article_id.trim();
        if article_id.is_empty() {
            return Err(AppError::Validation(
                "articleId must not be empty".to_string(),
            ));
        }

        Ok((article_id.to_string(), self.is_favorite))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreArchivedArticleParams {
    pub article_id: String,
}

impl RestoreArchivedArticleParams {
    pub fn validated_article_id(&self) -> Result<String, AppError> {
        let article_id = self.article_id.trim();
        if article_id.is_empty()
            || !article_id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err(AppError::Validation(
                "articleId contains unsupported characters".to_string(),
            ));
        }
        Ok(article_id.to_string())
    }
}

/// 月別アーカイブ記事一覧の引数。年月だけを受け取り、パスやファイル名は受け取らない。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListArchiveMonthArticlesParams {
    pub month: String,
}

impl ListArchiveMonthArticlesParams {
    pub fn validated_month(&self) -> Result<String, AppError> {
        let month = self.month.trim();
        if !is_valid_archive_month(month) {
            return Err(AppError::Validation(
                "month must be in YYYY-MM format".to_string(),
            ));
        }
        Ok(month.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_archive_candidate, is_within_title_dedupe_window, normalize_title_for_dedupe,
        ArticleDedupeKeys, ArticleHistoryFilter, GetArticleDetailParams,
        GetRecommendedArticlesParams, ListArchiveMonthArticlesParams, ListArticleHistoryParams,
        RestoreArchivedArticleParams, UpdateArticleFavoriteParams,
    };
    use chrono::{TimeZone, Utc};

    #[test]
    fn normalize_title_absorbs_surrounding_repeated_and_fullwidth_spaces() {
        let expected = "新製品 発表";
        assert_eq!(normalize_title_for_dedupe("  新製品 発表 "), expected);
        assert_eq!(normalize_title_for_dedupe("新製品   発表"), expected);
        assert_eq!(
            normalize_title_for_dedupe("\u{3000}新製品\u{3000}発表\u{3000}"),
            expected
        );
        assert_eq!(
            normalize_title_for_dedupe("新製品 \u{3000}\t発表"),
            expected
        );
        // 大文字小文字などは同一視しない。
        assert_eq!(normalize_title_for_dedupe(" Apple  News "), "Apple News");
    }

    #[test]
    fn dedupe_keys_match_same_source_and_normalized_title_regardless_of_url() {
        let mut keys = ArticleDedupeKeys::default();
        keys.insert("news_a".to_string(), "出典A", "同じ\u{3000}タイトル");

        assert!(keys.is_duplicate("news_a", "出典B", "別タイトル"));
        assert!(keys.is_duplicate("news_b", "出典A", " 同じ  タイトル "));
        // 出典が異なる同一タイトルは別記事。
        assert!(!keys.is_duplicate("news_b", "出典B", "同じ タイトル"));
        assert!(!keys.is_duplicate("news_b", "出典A", "違う タイトル"));
        // 大文字小文字の違いは同一視しない。
        keys.insert("news_c".to_string(), "出典A", "Apple News");
        assert!(!keys.is_duplicate("news_d", "出典A", "apple news"));
    }

    #[test]
    fn dedupe_keys_ignore_blank_titles() {
        let mut keys = ArticleDedupeKeys::default();
        keys.insert("news_a".to_string(), "出典A", " \u{3000} ");
        assert!(!keys.is_duplicate("news_b", "出典A", ""));
    }

    #[test]
    fn title_dedupe_window_includes_only_last_seven_days() {
        let now = Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap();
        // 境界（ちょうど7日前）は含める。
        assert!(is_within_title_dedupe_window(
            "2026-09-30T12:00:00Z",
            "",
            now
        ));
        assert!(is_within_title_dedupe_window(
            "2026-10-06T21:00:00+09:00",
            "",
            now
        ));
        // 7日を1秒でも過ぎたら比較対象外。
        assert!(!is_within_title_dedupe_window(
            "2026-09-30T11:59:59Z",
            "2026-10-07T00:00:00Z",
            now
        ));
        // 未来時刻（時計ずれ）は直近とみなす。
        assert!(is_within_title_dedupe_window(
            "2026-10-08T00:00:00Z",
            "",
            now
        ));
    }

    #[test]
    fn title_dedupe_window_falls_back_to_published_at_and_excludes_unknown_time() {
        let now = Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap();
        // 取得日時が解釈できなければ公開日時で判定する。
        assert!(is_within_title_dedupe_window(
            "",
            "2026-10-05T00:00:00Z",
            now
        ));
        assert!(!is_within_title_dedupe_window(
            "不明",
            "2026-09-01T00:00:00Z",
            now
        ));
        // どちらも解釈できなければ比較対象に含めない。
        assert!(!is_within_title_dedupe_window("", "1時間前", now));
    }

    #[test]
    fn read_state_advances_only_forward() {
        use super::ArticleReadState::{DetailViewed, Previewed, Unread};

        assert!(Unread.can_advance_to(&Previewed));
        assert!(Unread.can_advance_to(&DetailViewed));
        assert!(Previewed.can_advance_to(&DetailViewed));
        // 同じ状態・後退方向は更新しない。
        assert!(!Previewed.can_advance_to(&Previewed));
        assert!(!DetailViewed.can_advance_to(&Previewed));
        assert!(!DetailViewed.can_advance_to(&Unread));
        assert!(!Previewed.can_advance_to(&Unread));
    }

    #[test]
    fn normalized_limit_defaults_to_twenty() {
        let params = GetRecommendedArticlesParams { limit: None };
        assert_eq!(params.normalized_limit().unwrap(), 20);
    }

    #[test]
    fn normalized_limit_accepts_valid_range() {
        let params = GetRecommendedArticlesParams { limit: Some(10) };
        assert_eq!(params.normalized_limit().unwrap(), 10);
    }

    #[test]
    fn normalized_limit_rejects_out_of_range_values() {
        let zero = GetRecommendedArticlesParams { limit: Some(0) };
        assert!(zero.normalized_limit().is_err());

        let too_large = GetRecommendedArticlesParams { limit: Some(51) };
        assert!(too_large.normalized_limit().is_err());
    }

    #[test]
    fn history_limit_defaults_to_one_hundred() {
        let params = ListArticleHistoryParams::default();
        assert_eq!(params.normalized_limit().unwrap(), 100);
    }

    #[test]
    fn history_limit_rejects_out_of_range_values() {
        let zero = ListArticleHistoryParams {
            limit: Some(0),
            filter: None,
        };
        assert!(zero.normalized_limit().is_err());

        let too_large = ListArticleHistoryParams {
            limit: Some(201),
            filter: None,
        };
        assert!(too_large.normalized_limit().is_err());
    }

    #[test]
    fn history_filter_defaults_to_all() {
        let params = ListArticleHistoryParams {
            limit: None,
            filter: None,
        };

        assert_eq!(params.normalized_filter(), ArticleHistoryFilter::All);
    }

    #[test]
    fn validated_article_id_trims_whitespace() {
        let params = GetArticleDetailParams {
            article_id: "  article-001  ".to_string(),
        };

        assert_eq!(params.validated_article_id().unwrap(), "article-001");
    }

    #[test]
    fn validated_article_id_rejects_empty_value() {
        let params = GetArticleDetailParams {
            article_id: "   ".to_string(),
        };

        assert!(params.validated_article_id().is_err());
    }

    #[test]
    fn update_article_favorite_params_trim_values() {
        let params = UpdateArticleFavoriteParams {
            article_id: " article-001 ".to_string(),
            is_favorite: true,
        };

        let (article_id, is_favorite) = params.validated_inputs().unwrap();
        assert_eq!(article_id, "article-001");
        assert!(is_favorite);
    }

    #[test]
    fn update_article_favorite_params_reject_empty_value() {
        let params = UpdateArticleFavoriteParams {
            article_id: " ".to_string(),
            is_favorite: false,
        };

        assert!(params.validated_inputs().is_err());
    }

    #[test]
    fn list_archive_month_articles_params_accept_only_year_month() {
        let params = ListArchiveMonthArticlesParams {
            month: " 2026-09 ".to_string(),
        };
        assert_eq!(params.validated_month().unwrap(), "2026-09");

        for invalid in [
            "",
            "2026-9",
            "2026-00",
            "2026-13",
            "202609",
            "2026/09",
            "../2026-09",
            "2026-09.zip",
            "2026-09/../../x",
            "２０２６-０９",
        ] {
            let params = ListArchiveMonthArticlesParams {
                month: invalid.to_string(),
            };
            assert!(
                params.validated_month().is_err(),
                "{invalid} should be rejected"
            );
        }
    }

    #[test]
    fn restore_archived_article_params_validate_and_trim() {
        let params = RestoreArchivedArticleParams {
            article_id: " article-001 ".to_string(),
        };
        assert_eq!(params.validated_article_id().unwrap(), "article-001");

        let invalid = RestoreArchivedArticleParams {
            article_id: "../article".to_string(),
        };
        assert!(invalid.validated_article_id().is_err());
    }

    #[test]
    fn is_archive_candidate_true_for_old_plain_article() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        // 約2か月前・非お気に入り・非archived
        assert!(is_archive_candidate(
            "2026-05-10T09:00:00Z",
            false,
            false,
            now
        ));
    }

    #[test]
    fn is_archive_candidate_false_for_recent_article() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        // 5日前は候補でない
        assert!(!is_archive_candidate(
            "2026-07-10T09:00:00Z",
            false,
            false,
            now
        ));
    }

    #[test]
    fn is_archive_candidate_false_when_favorite_or_archived() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        // 古くてもお気に入りは除外
        assert!(!is_archive_candidate(
            "2026-01-01T00:00:00Z",
            true,
            false,
            now
        ));
        // 古くてもarchived済みは除外
        assert!(!is_archive_candidate(
            "2026-01-01T00:00:00Z",
            false,
            true,
            now
        ));
    }

    #[test]
    fn is_archive_candidate_boundary_at_thirty_days() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        // ちょうど30日前は候補（<=）
        assert!(is_archive_candidate(
            "2026-06-15T00:00:00Z",
            false,
            false,
            now
        ));
        // 29日前は候補でない
        assert!(!is_archive_candidate(
            "2026-06-16T00:00:00Z",
            false,
            false,
            now
        ));
    }

    #[test]
    fn is_archive_candidate_false_for_unparseable_fetched_at() {
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        assert!(!is_archive_candidate("not-a-date", false, false, now));
    }
}
