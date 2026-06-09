use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::article::{
    ArticleDetailDto, ArticleHistoryFilter, ArticleHistoryItemDto, ArticleReadState,
    ArticleSummaryDto, ArticleSummaryUpdate, FavoriteUpdateResult, FetchedArticle,
};
use crate::error::AppError;
use crate::paths::AppPaths;

const FRONT_MATTER_DELIMITER: &str = "---";

#[derive(Debug, Clone)]
pub struct ArticleRepository {
    article_favorites_path: PathBuf,
    article_news_dir: PathBuf,
}

impl ArticleRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            article_favorites_path: paths.article_favorites_path.clone(),
            article_news_dir: paths.article_news_dir.clone(),
        }
    }

    #[cfg(test)]
    fn with_paths(article_news_dir: PathBuf, article_favorites_path: PathBuf) -> Self {
        Self {
            article_favorites_path,
            article_news_dir,
        }
    }

    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.article_news_dir)?;
        if news_dir_contains_markdown_files(&self.article_news_dir)? {
            return Ok(());
        }

        for article in seed_articles() {
            self.save_article_record(&article)?;
        }

        Ok(())
    }

    pub fn list_recommended(&self, limit: usize) -> Result<Vec<ArticleSummaryDto>, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        let mut articles = self.load_article_records()?;
        articles.sort_by(compare_article_records);

        Ok(articles
            .into_iter()
            .map(|article| {
                article.to_summary_dto(is_effectively_favorite(&article, &favorite_store))
            })
            .take(limit)
            .collect())
    }

    pub fn list_history(
        &self,
        filter: ArticleHistoryFilter,
        limit: usize,
    ) -> Result<Vec<ArticleHistoryItemDto>, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        let mut articles = self.load_article_records()?;
        articles.sort_by(compare_history_records);

        Ok(articles
            .into_iter()
            .filter_map(|article| {
                let is_favorite = is_effectively_favorite(&article, &favorite_store);
                matches_history_filter(&article, is_favorite, &filter)
                    .then(|| article.to_history_item_dto(is_favorite))
            })
            .take(limit)
            .collect())
    }

    pub fn get_article_detail(&self, article_id: &str) -> Result<ArticleDetailDto, AppError> {
        let favorite_store = self.load_favorite_store_or_default()?;
        let article = self.find_article_record(article_id)?;
        Ok(article.to_detail_dto(is_effectively_favorite(&article, &favorite_store)))
    }

    pub fn update_article_favorite(
        &self,
        article_id: &str,
        is_favorite: bool,
    ) -> Result<FavoriteUpdateResult, AppError> {
        let mut article = self.find_article_record(article_id)?;
        article.favorite = is_favorite;
        self.save_article_record(&article)?;

        let mut favorite_store = self.load_favorite_store_or_default()?;
        favorite_store.set(article_id, is_favorite);
        if let Err(error) = self.save_favorite_store(&favorite_store) {
            log::warn!("Failed to persist favorite override JSON: {error}");
        }

        Ok(FavoriteUpdateResult {
            article_id: article_id.to_string(),
            is_favorite,
        })
    }

    /// 生成済み要約を記事Markdownへ永続化する（B-4）。
    /// 既存記事をロードして要約系フィールド＋メタ（summarized / summary_generated_at / ai_provider）を
    /// 更新し Markdownへ保存する。再表示は get_article_detail がこの保存値を返す（=キャッシュ）。
    pub fn update_article_summary(
        &self,
        article_id: &str,
        update: ArticleSummaryUpdate,
    ) -> Result<(), AppError> {
        let mut article = self.find_article_record(article_id)?;
        article.summary = Some(update.summary);
        article.yuuko_explanation = Some(update.yuuko_explanation);
        article.focus_points = update.focus_points;
        article.yuuko_comment = Some(update.yuuko_comment);
        article.status.summarized = true;
        article.summary_generated_at = Some(update.generated_at);
        article.ai_provider = Some(update.ai_provider);
        self.save_article_record(&article)?;
        Ok(())
    }

    /// 既存記事の article_id 集合を返す（取得時の重複排除に使う）。
    pub fn existing_article_ids(&self) -> Result<HashSet<String>, AppError> {
        Ok(self
            .load_article_records()?
            .into_iter()
            .map(|article| article.article_id)
            .collect())
    }

    /// 取得済みの新規記事を保存する（内部Rust API・Tauri commandとして公開しない）。
    /// 既存 article_id はスキップして重複排除し、新規保存できた件数を返す。
    pub fn save_fetched_articles(&self, articles: Vec<FetchedArticle>) -> Result<usize, AppError> {
        let mut existing = self.existing_article_ids()?;
        let mut saved = 0usize;

        for article in articles {
            // 同一 article_id は新規保存しない（既存記事の上書きを避ける）。
            if !existing.insert(article.article_id.clone()) {
                continue;
            }
            let record = PersistedArticleRecord::from_fetched(article);
            self.save_article_record(&record)?;
            saved += 1;
        }

        Ok(saved)
    }

    fn find_article_record(&self, article_id: &str) -> Result<PersistedArticleRecord, AppError> {
        self.load_article_records()?
            .into_iter()
            .find(|article| article.article_id == article_id)
            .ok_or_else(|| AppError::NotFound(format!("article not found: {article_id}")))
    }

    fn load_article_records(&self) -> Result<Vec<PersistedArticleRecord>, AppError> {
        let mut markdown_paths = Vec::new();
        collect_markdown_files(&self.article_news_dir, &mut markdown_paths)?;
        markdown_paths.sort();

        markdown_paths
            .into_iter()
            .map(|path| self.load_article_record(&path))
            .collect()
    }

    fn load_article_record(&self, path: &Path) -> Result<PersistedArticleRecord, AppError> {
        let raw = std::fs::read_to_string(path)?;
        let (front_matter_raw, body_raw) = split_front_matter(&raw, path)?;
        let front_matter: PersistedArticleFrontMatter = serde_yaml::from_str(&front_matter_raw)
            .map_err(|error| {
                AppError::Parse(format!(
                    "failed to parse article front matter '{}': {error}",
                    path.display()
                ))
            })?;
        let sections = parse_article_body(&body_raw);
        PersistedArticleRecord::from_parts(front_matter, sections)
    }

    fn save_article_record(&self, article: &PersistedArticleRecord) -> Result<(), AppError> {
        let path = self.article_path(article);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let payload = serialize_article_markdown(article)?;
        atomic_write(&path, payload.as_bytes(), "article markdown")
    }

    fn article_path(&self, article: &PersistedArticleRecord) -> PathBuf {
        self.article_news_dir
            .join(article.month_bucket())
            .join(format!("{}.md", article.article_id))
    }

    fn load_favorite_store_or_default(&self) -> Result<ArticleFavoriteStore, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.article_favorites_path.exists() {
            return Ok(ArticleFavoriteStore::default());
        }

        let raw = std::fs::read_to_string(&self.article_favorites_path)?;
        let mut store = serde_json::from_str::<ArticleFavoriteStore>(&raw)?;
        if store.version == 0 {
            store.version = 1;
        }
        Ok(store)
    }

    fn save_favorite_store(&self, store: &ArticleFavoriteStore) -> Result<(), AppError> {
        if let Some(parent) = self.article_favorites_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let payload = serde_json::to_vec_pretty(store)?;
        atomic_write(&self.article_favorites_path, &payload, "article favorites")
    }

    fn restore_backup_if_primary_missing(&self) {
        if self.article_favorites_path.exists() {
            return;
        }

        let backup_path = self.article_favorites_path.with_extension("json.bak");
        if !backup_path.exists() {
            return;
        }

        log::warn!("article favorites file is missing. attempting backup restore.");
        if let Err(error) = std::fs::rename(&backup_path, &self.article_favorites_path) {
            log::error!("Failed to restore article favorites backup: {error}");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct ArticleFavoriteStore {
    version: u32,
    favorite_article_ids: Vec<String>,
}

impl Default for ArticleFavoriteStore {
    fn default() -> Self {
        Self {
            version: 1,
            favorite_article_ids: Vec::new(),
        }
    }
}

impl ArticleFavoriteStore {
    fn contains(&self, article_id: &str) -> bool {
        self.favorite_article_ids
            .iter()
            .any(|favorite_article_id| favorite_article_id == article_id)
    }

    fn set(&mut self, article_id: &str, is_favorite: bool) {
        if is_favorite {
            if !self.contains(article_id) {
                self.favorite_article_ids.push(article_id.to_string());
            }
            return;
        }

        self.favorite_article_ids
            .retain(|favorite_article_id| favorite_article_id != article_id);
    }
}

#[derive(Debug, Clone)]
struct PersistedArticleRecord {
    version: u32,
    article_id: String,
    title: String,
    source_name: String,
    source_key: String,
    original_url: String,
    fetched_at: String,
    published_at_text: String,
    genre: String,
    tags: Vec<String>,
    status: PersistedArticleStatus,
    read_state: ArticleReadState,
    favorite: bool,
    is_archived: bool,
    recommendation_score: f32,
    summary_generated_at: Option<String>,
    ai_provider: Option<String>,
    content_hash: Option<String>,
    excerpt: Option<String>,
    summary: Option<String>,
    yuuko_explanation: Option<String>,
    focus_points: Vec<String>,
    yuuko_comment: Option<String>,
    keyword_candidates: Vec<String>,
}

impl PersistedArticleRecord {
    fn from_parts(
        front_matter: PersistedArticleFrontMatter,
        sections: ArticleBodySections,
    ) -> Result<Self, AppError> {
        let source_key = if front_matter.source_key.trim().is_empty() {
            build_source_key(&front_matter.source_name)
        } else {
            front_matter.source_key
        };

        let published_at_text = front_matter
            .published_at
            .clone()
            .unwrap_or_else(|| front_matter.fetched_at.clone());

        Ok(Self {
            version: normalize_version(front_matter.version),
            article_id: required_field(front_matter.article_id, "articleId")?,
            title: required_field(front_matter.title, "title")?,
            source_name: required_field(front_matter.source_name, "sourceName")?,
            source_key,
            original_url: required_field(front_matter.url, "url")?,
            fetched_at: required_field(front_matter.fetched_at, "fetchedAt")?,
            published_at_text,
            genre: required_field(front_matter.genre, "genre")?,
            tags: front_matter.tags,
            status: front_matter.status,
            read_state: front_matter.read_state,
            favorite: front_matter.favorite,
            is_archived: front_matter.archived.unwrap_or(matches!(
                front_matter.archive_state,
                Some(PersistedArchiveState::Archived)
            )),
            recommendation_score: front_matter.recommendation_score,
            summary_generated_at: front_matter.summary_generated_at,
            ai_provider: front_matter.ai_provider,
            content_hash: front_matter.content_hash,
            excerpt: sections.excerpt,
            summary: sections.summary,
            yuuko_explanation: sections.yuuko_explanation,
            focus_points: sections.focus_points,
            yuuko_comment: sections.yuuko_comment,
            keyword_candidates: sections.keyword_candidates,
        })
    }

    /// 取得パイプラインの `FetchedArticle` から永続化レコードを構築する。
    /// AI要約前の段階なので summary 系は空、status は取得・抽出済みを反映する。
    fn from_fetched(article: FetchedArticle) -> Self {
        let source_key = build_source_key(&article.source_name);
        let html_extracted = article.excerpt.is_some();
        Self {
            version: 1,
            article_id: article.article_id,
            title: article.title,
            source_name: article.source_name,
            source_key,
            original_url: article.original_url,
            fetched_at: article.fetched_at,
            published_at_text: article.published_at_text,
            genre: article.genre,
            tags: article.tags,
            status: PersistedArticleStatus {
                fetched: true,
                html_extracted,
                markdown_generated: true,
                summarized: false,
                recommended: true,
                introduced_by_yuuko: false,
                archived: false,
            },
            read_state: article.read_state,
            favorite: false,
            is_archived: false,
            recommendation_score: article.recommendation_score,
            summary_generated_at: None,
            ai_provider: None,
            content_hash: None,
            excerpt: article.excerpt,
            summary: None,
            yuuko_explanation: None,
            focus_points: Vec::new(),
            yuuko_comment: None,
            keyword_candidates: Vec::new(),
        }
    }

    fn to_summary_dto(&self, is_favorite: bool) -> ArticleSummaryDto {
        ArticleSummaryDto {
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            published_at_text: self.published_at_text.clone(),
            genre: self.genre.clone(),
            summary: self.summary.clone().or_else(|| self.excerpt.clone()),
            is_favorite,
            read_state: self.read_state.clone(),
            recommendation_score: self.recommendation_score,
        }
    }

    fn to_history_item_dto(&self, is_favorite: bool) -> ArticleHistoryItemDto {
        ArticleHistoryItemDto {
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            published_at_text: self.published_at_text.clone(),
            fetched_at: self.fetched_at.clone(),
            genre: self.genre.clone(),
            summary: self.summary.clone().or_else(|| self.excerpt.clone()),
            is_favorite,
            read_state: self.read_state.clone(),
            is_archived: self.is_archived,
            recommendation_score: self.recommendation_score,
        }
    }

    fn to_detail_dto(&self, is_favorite: bool) -> ArticleDetailDto {
        ArticleDetailDto {
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            original_url: self.original_url.clone(),
            published_at_text: self.published_at_text.clone(),
            genre: self.genre.clone(),
            summary: self.summary.clone().or_else(|| self.excerpt.clone()),
            excerpt: self.excerpt.clone(),
            yuuko_explanation: self.yuuko_explanation.clone(),
            focus_points: self.focus_points.clone(),
            yuuko_comment: self.yuuko_comment.clone(),
            is_favorite,
            keyword_candidates: self.keyword_candidates.clone(),
        }
    }

    fn to_front_matter(&self) -> PersistedArticleFrontMatter {
        PersistedArticleFrontMatter {
            version: self.version,
            article_id: self.article_id.clone(),
            title: self.title.clone(),
            source_name: self.source_name.clone(),
            source_key: self.source_key.clone(),
            url: self.original_url.clone(),
            fetched_at: self.fetched_at.clone(),
            published_at: Some(self.published_at_text.clone()),
            genre: self.genre.clone(),
            tags: self.tags.clone(),
            status: self.status.clone(),
            read_state: self.read_state.clone(),
            favorite: self.favorite,
            archive_state: None,
            archived: Some(self.is_archived),
            recommendation_score: self.recommendation_score,
            summary_generated_at: self.summary_generated_at.clone(),
            ai_provider: self.ai_provider.clone(),
            content_hash: self.content_hash.clone(),
        }
    }

    fn month_bucket(&self) -> String {
        month_bucket_from_text(&self.published_at_text)
            .or_else(|| month_bucket_from_text(&self.fetched_at))
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedArticleFrontMatter {
    #[serde(default = "default_version")]
    version: u32,
    article_id: String,
    title: String,
    source_name: String,
    #[serde(default)]
    source_key: String,
    url: String,
    fetched_at: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default, alias = "category")]
    genre: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    status: PersistedArticleStatus,
    #[serde(default = "default_read_state")]
    read_state: ArticleReadState,
    #[serde(default)]
    favorite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archive_state: Option<PersistedArchiveState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archived: Option<bool>,
    #[serde(default)]
    recommendation_score: f32,
    #[serde(default)]
    summary_generated_at: Option<String>,
    #[serde(default)]
    ai_provider: Option<String>,
    #[serde(default)]
    content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedArticleStatus {
    fetched: bool,
    html_extracted: bool,
    markdown_generated: bool,
    summarized: bool,
    recommended: bool,
    introduced_by_yuuko: bool,
    archived: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistedArchiveState {
    Active,
    Archived,
}

#[derive(Debug, Default)]
struct ArticleBodySections {
    excerpt: Option<String>,
    summary: Option<String>,
    yuuko_explanation: Option<String>,
    focus_points: Vec<String>,
    yuuko_comment: Option<String>,
    keyword_candidates: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum ArticleBodySection {
    Excerpt,
    Summary,
    Explanation,
    FocusPoints,
    Comment,
    Keywords,
}

impl ArticleBodySection {
    fn from_heading(line: &str) -> Option<Self> {
        match line.trim() {
            "## 本文抜粋" => Some(Self::Excerpt),
            "## AI要約" => Some(Self::Summary),
            "## ゆうこの用語解説" => Some(Self::Explanation),
            "## 注目ポイント" => Some(Self::FocusPoints),
            "## ゆうこの一言" => Some(Self::Comment),
            "## 解説対象キーワード" => Some(Self::Keywords),
            _ => None,
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Excerpt => "本文抜粋",
            Self::Summary => "AI要約",
            Self::Explanation => "ゆうこの用語解説",
            Self::FocusPoints => "注目ポイント",
            Self::Comment => "ゆうこの一言",
            Self::Keywords => "解説対象キーワード",
        }
    }
}

fn news_dir_contains_markdown_files(dir: &Path) -> Result<bool, AppError> {
    if !dir.exists() {
        return Ok(false);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if news_dir_contains_markdown_files(&path)? {
                return Ok(true);
            }
            continue;
        }

        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("md"))
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), AppError> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
            continue;
        }

        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("md"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn split_front_matter(raw: &str, path: &Path) -> Result<(String, String), AppError> {
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some(FRONT_MATTER_DELIMITER) {
        return Err(AppError::Parse(format!(
            "article markdown '{}' is missing YAML front matter",
            path.display()
        )));
    }

    let mut front_matter_lines = Vec::new();
    let mut found_closing_delimiter = false;
    for line in &mut lines {
        if line.trim() == FRONT_MATTER_DELIMITER {
            found_closing_delimiter = true;
            break;
        }
        front_matter_lines.push(line);
    }

    if !found_closing_delimiter {
        return Err(AppError::Parse(format!(
            "article markdown '{}' has an unterminated YAML front matter block",
            path.display()
        )));
    }

    Ok((
        front_matter_lines.join("\n"),
        lines.collect::<Vec<_>>().join("\n"),
    ))
}

fn parse_article_body(body: &str) -> ArticleBodySections {
    let mut sections = ArticleBodySections::default();
    let mut current_section = None;
    let mut current_lines = Vec::new();

    for line in body.lines() {
        if let Some(next_section) = ArticleBodySection::from_heading(line) {
            flush_body_section(&mut sections, current_section, &current_lines);
            current_section = Some(next_section);
            current_lines.clear();
            continue;
        }

        if current_section.is_some() {
            current_lines.push(line.to_string());
        }
    }

    flush_body_section(&mut sections, current_section, &current_lines);
    sections
}

fn flush_body_section(
    sections: &mut ArticleBodySections,
    section: Option<ArticleBodySection>,
    lines: &[String],
) {
    let Some(section) = section else {
        return;
    };

    match section {
        ArticleBodySection::Excerpt => {
            sections.excerpt = normalize_text_block(lines);
        }
        ArticleBodySection::Summary => {
            sections.summary = normalize_text_block(lines);
        }
        ArticleBodySection::Explanation => {
            sections.yuuko_explanation = normalize_text_block(lines);
        }
        ArticleBodySection::Comment => {
            sections.yuuko_comment = normalize_text_block(lines);
        }
        ArticleBodySection::FocusPoints => {
            sections.focus_points = normalize_list_block(lines);
        }
        ArticleBodySection::Keywords => {
            sections.keyword_candidates = normalize_list_block(lines);
        }
    }
}

fn normalize_text_block(lines: &[String]) -> Option<String> {
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn normalize_list_block(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            let value = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .unwrap_or(trimmed)
                .trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        })
        .collect()
}

fn serialize_article_markdown(article: &PersistedArticleRecord) -> Result<String, AppError> {
    let mut front_matter = serde_yaml::to_string(&article.to_front_matter()).map_err(|error| {
        AppError::Parse(format!("failed to serialize article front matter: {error}"))
    })?;
    if let Some(stripped) = front_matter.strip_prefix("---\n") {
        front_matter = stripped.to_string();
    }

    let body = compose_article_body(article);
    Ok(format!(
        "{FRONT_MATTER_DELIMITER}\n{front_matter}{FRONT_MATTER_DELIMITER}\n\n{body}"
    ))
}

fn compose_article_body(article: &PersistedArticleRecord) -> String {
    let mut sections = Vec::new();

    push_text_section(
        &mut sections,
        ArticleBodySection::Excerpt,
        article.excerpt.as_deref(),
    );
    push_text_section(
        &mut sections,
        ArticleBodySection::Summary,
        article.summary.as_deref(),
    );
    push_text_section(
        &mut sections,
        ArticleBodySection::Explanation,
        article.yuuko_explanation.as_deref(),
    );
    push_list_section(
        &mut sections,
        ArticleBodySection::FocusPoints,
        &article.focus_points,
    );
    push_text_section(
        &mut sections,
        ArticleBodySection::Comment,
        article.yuuko_comment.as_deref(),
    );
    push_list_section(
        &mut sections,
        ArticleBodySection::Keywords,
        &article.keyword_candidates,
    );

    sections.join("\n\n")
}

fn push_text_section(buffer: &mut Vec<String>, section: ArticleBodySection, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    buffer.push(format!("## {}\n{}", section.heading(), value));
}

fn push_list_section(buffer: &mut Vec<String>, section: ArticleBodySection, values: &[String]) {
    if values.is_empty() {
        return;
    }

    let items = values
        .iter()
        .map(|value| format!("- {}", value.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    buffer.push(format!("## {}\n{}", section.heading(), items));
}

fn compare_article_records(
    left: &PersistedArticleRecord,
    right: &PersistedArticleRecord,
) -> Ordering {
    right
        .recommendation_score
        .partial_cmp(&left.recommendation_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| right.fetched_at.cmp(&left.fetched_at))
        .then_with(|| left.article_id.cmp(&right.article_id))
}

fn compare_history_records(
    left: &PersistedArticleRecord,
    right: &PersistedArticleRecord,
) -> Ordering {
    right
        .fetched_at
        .cmp(&left.fetched_at)
        .then_with(|| right.published_at_text.cmp(&left.published_at_text))
        .then_with(|| left.article_id.cmp(&right.article_id))
}

fn matches_history_filter(
    article: &PersistedArticleRecord,
    is_favorite: bool,
    filter: &ArticleHistoryFilter,
) -> bool {
    match filter {
        ArticleHistoryFilter::All => true,
        ArticleHistoryFilter::Unread => article.read_state == ArticleReadState::Unread,
        ArticleHistoryFilter::Read => article.read_state != ArticleReadState::Unread,
        ArticleHistoryFilter::Favorite => is_favorite,
        ArticleHistoryFilter::Archived => article.is_archived,
    }
}

fn is_effectively_favorite(
    article: &PersistedArticleRecord,
    favorite_store: &ArticleFavoriteStore,
) -> bool {
    article.favorite || favorite_store.contains(&article.article_id)
}

fn month_bucket_from_text(value: &str) -> Option<String> {
    let digits = value
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(6)
        .collect::<String>();
    if digits.len() == 6 {
        Some(digits)
    } else {
        None
    }
}

fn build_source_key(source_name: &str) -> String {
    let mut key = String::new();
    let mut last_was_separator = false;

    for ch in source_name.chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch.to_ascii_lowercase());
            last_was_separator = false;
            continue;
        }

        if !last_was_separator && !key.is_empty() {
            key.push('_');
            last_was_separator = true;
        }
    }

    key.trim_matches('_')
        .to_string()
        .chars()
        .take(32)
        .collect::<String>()
}

fn required_field(value: String, field_name: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Parse(format!(
            "article front matter field '{field_name}' must not be empty"
        )));
    }

    Ok(trimmed.to_string())
}

fn normalize_version(version: u32) -> u32 {
    if version == 0 {
        1
    } else {
        version
    }
}

fn default_version() -> u32 {
    1
}

fn default_read_state() -> ArticleReadState {
    ArticleReadState::Unread
}

fn atomic_write(path: &Path, payload: &[u8], label: &str) -> Result<(), AppError> {
    let temp_path = with_extension_suffix(path, "tmp");
    let backup_path = with_extension_suffix(path, "bak");
    std::fs::write(&temp_path, payload)?;

    let had_existing = path.exists();
    if had_existing {
        if backup_path.exists() {
            std::fs::remove_file(&backup_path)?;
        }
        std::fs::rename(path, &backup_path)?;
    }

    match std::fs::rename(&temp_path, path) {
        Ok(()) => {
            if had_existing && backup_path.exists() {
                if let Err(error) = std::fs::remove_file(&backup_path) {
                    log::warn!("Failed to remove {label} backup: {error}");
                }
            }
            Ok(())
        }
        Err(error) => {
            log::error!("Failed to promote temporary {label} file: {error}");

            if had_existing && backup_path.exists() {
                if let Err(restore_error) = std::fs::rename(&backup_path, path) {
                    log::error!("Failed to restore {label} backup: {restore_error}");
                }
            }

            if temp_path.exists() {
                let _ = std::fs::remove_file(&temp_path);
            }

            Err(error.into())
        }
    }
}

fn with_extension_suffix(path: &Path, suffix: &str) -> PathBuf {
    let base_extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.{suffix}"))
        .unwrap_or_else(|| suffix.to_string());
    path.with_extension(base_extension)
}

fn seed_articles() -> Vec<PersistedArticleRecord> {
    vec![
        PersistedArticleRecord {
            version: 1,
            article_id: "article-001".to_string(),
            title: "生成AIスタートアップの資金調達が活発化".to_string(),
            source_name: "TechCrunch Japan".to_string(),
            source_key: "techcrunch_japan".to_string(),
            original_url: "https://example.com/articles/article-001".to_string(),
            fetched_at: "2026-06-04T09:10:00+09:00".to_string(),
            published_at_text: "5分前".to_string(),
            genre: "AI・テクノロジー".to_string(),
            tags: vec!["AI".to_string(), "生成AI".to_string(), "資金調達".to_string()],
            status: PersistedArticleStatus {
                fetched: true,
                html_extracted: true,
                markdown_generated: true,
                summarized: true,
                recommended: true,
                introduced_by_yuuko: false,
                archived: false,
            },
            read_state: ArticleReadState::Unread,
            favorite: false,
            is_archived: false,
            recommendation_score: 0.92,
            summary_generated_at: Some("2026-06-04T09:12:00+09:00".to_string()),
            ai_provider: Some("mock".to_string()),
            content_hash: Some("content-hash-001".to_string()),
            excerpt: Some(
                "複数の生成AIスタートアップが国内外で大型の資金調達を発表し、企業向け活用の広がりが改めて注目されています。".to_string(),
            ),
            summary: Some(
                "生成AIを活用するスタートアップへの投資が再び活発になっており、法人向け導入支援や運用最適化の分野に資金が集まっています。".to_string(),
            ),
            yuuko_explanation: Some(
                "この記事では、生成AIそのものよりも、それをどう実務に組み込むかを支える企業に期待が集まっている点が大切です。".to_string(),
            ),
            focus_points: vec![
                "投資対象がモデル開発だけでなく運用支援まで広がっている".to_string(),
                "法人導入の具体策を持つ企業が評価されやすい".to_string(),
                "生成AIの実装コストを下げるサービスが増えている".to_string(),
            ],
            yuuko_comment: Some(
                "技術そのものより、使いこなす仕組みに注目が移ってきたのが面白い流れですね。".to_string(),
            ),
            keyword_candidates: vec![
                "生成AI".to_string(),
                "資金調達".to_string(),
                "法人導入".to_string(),
            ],
        },
        PersistedArticleRecord {
            version: 1,
            article_id: "article-002".to_string(),
            title: "SaaS企業が中堅市場向け新プランを発表".to_string(),
            source_name: "日経ビジネス".to_string(),
            source_key: "nikkei_business".to_string(),
            original_url: "https://example.com/articles/article-002".to_string(),
            fetched_at: "2026-06-04T08:40:00+09:00".to_string(),
            published_at_text: "1時間前".to_string(),
            genre: "ビジネス".to_string(),
            tags: vec!["SaaS".to_string(), "中堅企業".to_string()],
            status: PersistedArticleStatus {
                fetched: true,
                html_extracted: true,
                markdown_generated: true,
                summarized: true,
                recommended: true,
                introduced_by_yuuko: false,
                archived: false,
            },
            read_state: ArticleReadState::Unread,
            favorite: false,
            is_archived: false,
            recommendation_score: 0.84,
            summary_generated_at: Some("2026-06-04T08:45:00+09:00".to_string()),
            ai_provider: Some("mock".to_string()),
            content_hash: Some("content-hash-002".to_string()),
            excerpt: Some(
                "大企業向け中心だったSaaS製品を、中堅企業でも導入しやすい価格とサポート体制に見直す動きが広がっています。".to_string(),
            ),
            summary: Some(
                "SaaS各社が中堅企業向けに導入支援を強化し、価格だけでなく運用設計までセットで提供する新プランを打ち出しました。".to_string(),
            ),
            yuuko_explanation: Some(
                "単に安くするだけでなく、導入後にどう使い続けてもらうかまで含めて設計している点が重要です。".to_string(),
            ),
            focus_points: vec![
                "中堅企業向けに支援内容を明確化している".to_string(),
                "価格だけでなく運用支援を合わせて提供している".to_string(),
                "導入障壁を下げることが競争力になっている".to_string(),
            ],
            yuuko_comment: Some(
                "続けやすさまで商品に含める流れは、SaaSらしい成熟のしかたですね。".to_string(),
            ),
            keyword_candidates: vec![
                "SaaS".to_string(),
                "中堅企業".to_string(),
                "運用支援".to_string(),
            ],
        },
        PersistedArticleRecord {
            version: 1,
            article_id: "article-003".to_string(),
            title: "新型コンピュータ実験で省電力な推論手法を確認".to_string(),
            source_name: "ITmedia NEWS".to_string(),
            source_key: "itmedia_news".to_string(),
            original_url: "https://example.com/articles/article-003".to_string(),
            fetched_at: "2026-06-04T07:50:00+09:00".to_string(),
            published_at_text: "2時間前".to_string(),
            genre: "テクノロジー".to_string(),
            tags: vec!["半導体".to_string(), "推論".to_string(), "省電力".to_string()],
            status: PersistedArticleStatus {
                fetched: true,
                html_extracted: true,
                markdown_generated: true,
                summarized: true,
                recommended: true,
                introduced_by_yuuko: false,
                archived: false,
            },
            read_state: ArticleReadState::Previewed,
            favorite: false,
            is_archived: false,
            recommendation_score: 0.79,
            summary_generated_at: Some("2026-06-04T07:56:00+09:00".to_string()),
            ai_provider: Some("mock".to_string()),
            content_hash: Some("content-hash-003".to_string()),
            excerpt: Some(
                "研究チームは、新型コンピュータ構成でAI推論時の消費電力を抑えられる可能性を実験で示しました。".to_string(),
            ),
            summary: Some(
                "新しい計算構成を用いた推論実験で、省電力性と処理効率の両立が期待できる結果が報告されました。".to_string(),
            ),
            yuuko_explanation: Some(
                "推論は学習より身近な場面でたくさん実行されるので、電力効率の改善は実用面でとても効いてきます。".to_string(),
            ),
            focus_points: vec![
                "推論処理での省電力性が主な評価軸になっている".to_string(),
                "研究段階でも実運用を意識した測定が行われている".to_string(),
                "将来の端末実装にも影響する可能性がある".to_string(),
            ],
            yuuko_comment: Some(
                "派手さはなくても、日常的に動く技術ほど省電力化の価値が大きいですね。".to_string(),
            ),
            keyword_candidates: vec![
                "半導体".to_string(),
                "推論".to_string(),
                "省電力".to_string(),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::article::{ArticleHistoryFilter, ArticleReadState};

    use super::{
        month_bucket_from_text, ArticleRepository, ArticleSummaryUpdate, PersistedArticleRecord,
    };

    struct TestRepositoryContext {
        repository: ArticleRepository,
        news_dir: PathBuf,
        root_dir: PathBuf,
    }

    impl TestRepositoryContext {
        fn new() -> Self {
            let unique_suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root_dir = std::env::temp_dir().join(format!(
                "yuuko-article-tests-{}-{unique_suffix}",
                std::process::id()
            ));
            let news_dir = root_dir.join("news");
            let favorites_path = root_dir.join("favorites").join("article_favorites.json");
            std::fs::create_dir_all(&news_dir).unwrap();
            let repository = ArticleRepository::with_paths(news_dir.clone(), favorites_path);
            Self {
                repository,
                news_dir,
                root_dir,
            }
        }
    }

    impl Drop for TestRepositoryContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root_dir);
        }
    }

    #[test]
    fn initialize_default_if_missing_writes_seed_markdown_files() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let files = std::fs::read_dir(context.news_dir.join("202606"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(files, 3);
    }

    #[test]
    fn list_recommended_respects_limit() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let articles = context.repository.list_recommended(2).unwrap();
        assert_eq!(articles.len(), 2);
    }

    #[test]
    fn list_recommended_keeps_recommendation_order() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let articles = context.repository.list_recommended(3).unwrap();
        assert!(articles[0].recommendation_score >= articles[1].recommendation_score);
        assert!(articles[1].recommendation_score >= articles[2].recommendation_score);
    }

    #[test]
    fn list_history_returns_articles_by_fetched_at_desc() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::All, 3)
            .unwrap();

        assert_eq!(articles.len(), 3);
        assert_eq!(articles[0].article_id, "article-001");
        assert!(articles[0].fetched_at >= articles[1].fetched_at);
        assert!(articles[1].fetched_at >= articles[2].fetched_at);
    }

    #[test]
    fn list_history_respects_limit() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::All, 2)
            .unwrap();

        assert_eq!(articles.len(), 2);
    }

    #[test]
    fn list_history_filters_read_state() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let unread_articles = context
            .repository
            .list_history(ArticleHistoryFilter::Unread, 10)
            .unwrap();
        assert!(!unread_articles.is_empty());
        assert!(unread_articles
            .iter()
            .all(|article| article.read_state == ArticleReadState::Unread));

        let read_articles = context
            .repository
            .list_history(ArticleHistoryFilter::Read, 10)
            .unwrap();
        assert!(!read_articles.is_empty());
        assert!(read_articles
            .iter()
            .all(|article| article.read_state != ArticleReadState::Unread));
    }

    #[test]
    fn list_history_filters_favorites() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();
        context
            .repository
            .update_article_favorite("article-002", true)
            .unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::Favorite, 10)
            .unwrap();

        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].article_id, "article-002");
        assert!(articles[0].is_favorite);
    }

    #[test]
    fn list_history_filters_archived_articles() {
        let context = TestRepositoryContext::new();
        let archived_article = PersistedArticleRecord {
            article_id: "article-archived".to_string(),
            fetched_at: "2026-06-05T10:00:00+09:00".to_string(),
            is_archived: true,
            ..super::seed_articles().remove(0)
        };
        context
            .repository
            .save_article_record(&archived_article)
            .unwrap();

        let articles = context
            .repository
            .list_history(ArticleHistoryFilter::Archived, 10)
            .unwrap();

        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].article_id, "article-archived");
        assert!(articles[0].is_archived);
    }

    #[test]
    fn get_article_detail_returns_matching_article() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let article = context
            .repository
            .get_article_detail("article-002")
            .unwrap();

        assert_eq!(article.article_id, "article-002");
        assert_eq!(article.source_name, "日経ビジネス");
        assert!(!article.focus_points.is_empty());
    }

    #[test]
    fn get_article_detail_returns_not_found_for_unknown_id() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let error = context
            .repository
            .get_article_detail("article-999")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "not found: article not found: article-999"
        );
    }

    #[test]
    fn update_article_favorite_persists_state_for_list_and_detail() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let result = context
            .repository
            .update_article_favorite("article-001", true)
            .unwrap();
        assert!(result.is_favorite);

        let articles = context.repository.list_recommended(3).unwrap();
        assert!(
            articles
                .iter()
                .find(|article| article.article_id == "article-001")
                .unwrap()
                .is_favorite
        );

        let detail = context
            .repository
            .get_article_detail("article-001")
            .unwrap();
        assert!(detail.is_favorite);
    }

    #[test]
    fn update_article_summary_persists_into_markdown() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();

        let update = ArticleSummaryUpdate {
            summary: "新しいAI要約テキスト".to_string(),
            yuuko_explanation: "新しい再説明".to_string(),
            focus_points: vec!["観点A".to_string(), "観点B".to_string()],
            yuuko_comment: "新しい一言".to_string(),
            ai_provider: "gemini".to_string(),
            generated_at: "2026-06-08T00:00:00Z".to_string(),
        };
        context
            .repository
            .update_article_summary("article-002", update)
            .unwrap();

        // 再表示（DTO）で保存値がキャッシュとして返ることを確認。
        let detail = context
            .repository
            .get_article_detail("article-002")
            .unwrap();
        assert_eq!(detail.summary.as_deref(), Some("新しいAI要約テキスト"));
        assert_eq!(detail.yuuko_explanation.as_deref(), Some("新しい再説明"));
        assert_eq!(detail.yuuko_comment.as_deref(), Some("新しい一言"));
        assert_eq!(
            detail.focus_points,
            vec!["観点A".to_string(), "観点B".to_string()]
        );

        // front matter のメタ情報も永続化されていることを確認。
        let record = context
            .repository
            .find_article_record("article-002")
            .unwrap();
        assert!(record.status.summarized);
        assert_eq!(record.ai_provider.as_deref(), Some("gemini"));
        assert_eq!(
            record.summary_generated_at.as_deref(),
            Some("2026-06-08T00:00:00Z")
        );
    }

    #[test]
    fn update_article_favorite_removes_existing_favorite() {
        let context = TestRepositoryContext::new();
        context.repository.initialize_default_if_missing().unwrap();
        context
            .repository
            .update_article_favorite("article-001", true)
            .unwrap();

        let result = context
            .repository
            .update_article_favorite("article-001", false)
            .unwrap();
        assert!(!result.is_favorite);

        let detail = context
            .repository
            .get_article_detail("article-001")
            .unwrap();
        assert!(!detail.is_favorite);
    }

    #[test]
    fn save_article_record_uses_month_bucket_directory() {
        let context = TestRepositoryContext::new();
        let article = PersistedArticleRecord {
            article_id: "article-custom".to_string(),
            published_at_text: "2026-07-01T10:00:00+09:00".to_string(),
            fetched_at: "2026-07-01T10:05:00+09:00".to_string(),
            ..super::seed_articles().remove(0)
        };

        context.repository.save_article_record(&article).unwrap();

        assert!(context
            .news_dir
            .join("202607")
            .join("article-custom.md")
            .exists());
    }

    #[test]
    fn month_bucket_prefers_first_six_digits() {
        assert_eq!(
            month_bucket_from_text("2026-06-04T09:10:00+09:00").as_deref(),
            Some("202606")
        );
        assert_eq!(month_bucket_from_text("5分前"), None);
    }
}
