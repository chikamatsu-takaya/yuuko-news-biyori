use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::util::text_safety::contains_disallowed_control_char;

/// 辞書項目の種別。種別を持たない旧データ・旧呼び出しは「用語」として読む（`#[serde(default)]` 側で使う既定値）。
///
/// 旧種別 `key_point`（要点説明）は D21 で長文の要点説明が `phrase` に統合されたため廃止した（判断台帳 D144）。
/// 過去に `key_point` で保存された項目も辞書全体の読み込みを失敗させず、読み込み時に `phrase` として扱う
/// （`alias`）。ファイルは書き換えず、通常の保存（★・メモ・参照回数の更新など）のときに `phrase` で書き出される。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DictionaryEntryType {
    #[default]
    Term,
    #[serde(alias = "key_point")]
    Phrase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntryDto {
    pub entry_id: String,
    pub key_text: String,
    /// 種別（用語 / フレーズ）。保存パラメータで欠けているときは「用語」として受ける（旧画面との互換）。
    #[serde(rename = "type", default)]
    pub entry_type: DictionaryEntryType,
    pub short_explanation: String,
    pub detail_explanation: String,
    pub related_article_id: Option<String>,
    pub related_article_title: Option<String>,
    pub is_starred: bool,
    /// 辞書へ保存済みの項目か（保存済み辞書の命中・保存結果なら true、未保存の生成結果なら false）。
    /// ポップアップの「辞書保存済み」表示は ★（`is_starred`）ではなくこれで判定する（画面詳細設計書 §11.4）。
    /// 保存パラメータとして受け取るときは参照しない（保存側で決まる値のため、欠けていても既定 false で受ける）。
    #[serde(default)]
    pub saved_in_dictionary: bool,
    /// 解説の代わりの固定の案内（「この言葉はゆうこにも分からなかったよ。」「選んだ範囲が言葉の途中みたい…」）か。
    /// 案内は辞書へ保存しない（保存要求は拒否する）。画面は「辞書に保存」を出さず、友情ポイントも加算せず、流れ星のかけらも付与しない。
    /// 旧画面・保存済みの項目では欠けているので既定 false で受ける。
    #[serde(default)]
    pub is_notice: bool,
    /// 選んだ文字列が言葉の途中だったため語境界まで広げて解説したとき、元の選択文字列（広げていなければ None）。
    /// このとき `key_text` は広げた言葉で、画面の見出しはそちらにする。辞書へは保存しない（表示専用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntryListItemDto {
    pub entry_id: String,
    pub key_text: String,
    #[serde(rename = "type")]
    pub entry_type: DictionaryEntryType,
    pub short_explanation: String,
    pub detail_explanation: String,
    pub related_article_id: Option<String>,
    pub related_article_title: Option<String>,
    pub last_viewed_at_text: Option<String>,
    /// 作成日時（UNIX秒の文字列）。`last_viewed_at_text` と同様に表示用の整形は画面側で行う。
    /// 旧データで作成日時が空のときは `None`。
    pub created_at_text: Option<String>,
    /// 参照回数。旧データで欠けているときは 0（画面側で「—」扱い）。
    pub reference_count: u32,
    pub memo: Option<String>,
    pub is_starred: bool,
}

impl DictionaryEntryDto {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.entry_id.trim().is_empty() {
            return Err(AppError::Validation(
                "entryId must not be empty".to_string(),
            ));
        }

        if self.key_text.trim().is_empty() {
            return Err(AppError::Validation(
                "keyText must not be empty".to_string(),
            ));
        }

        if self.short_explanation.trim().is_empty() {
            return Err(AppError::Validation(
                "shortExplanation must not be empty".to_string(),
            ));
        }

        if self.detail_explanation.trim().is_empty() {
            return Err(AppError::Validation(
                "detailExplanation must not be empty".to_string(),
            ));
        }

        Ok(())
    }

    pub fn normalized_key_text(&self) -> String {
        normalize_text(&self.key_text)
    }
}

impl DictionaryEntryType {
    pub fn from_filter_value(value: &str) -> Result<Self, AppError> {
        match value.trim() {
            "term" => Ok(Self::Term),
            // 旧種別 `key_point` は `phrase` に統合済み（D144）。古い呼び出しでもフレーズとして絞り込む。
            "phrase" | "key_point" => Ok(Self::Phrase),
            // 入力値はエラー文言へ含めない（§16.3）。
            _ => Err(AppError::Validation(
                "type must be one of term, phrase".to_string(),
            )),
        }
    }
}

/// 選択語(selectedText)の Unicode 文字数上限（trim 後・chars 単位）。AIへ渡す入力の可変部分を
/// 明示的に有界化する。バイト長ではなく文字数で判定し、日本語・絵文字でも文字境界を壊さない。
/// 用語・フレーズ共通の全体上限（#213）。フレーズ対応（D21）でもこの値は変えない。
const TERM_EXPLANATION_SELECTED_TEXT_MAX_CHARS: usize = 200;

/// 「用語」とみなす選択の最大文字数（trim 後・chars 単位）。これより長い選択はフレーズ（文章）として扱う。
/// 正式名称の長い用語（「環太平洋パートナーシップに関する包括的及び先進的な協定」27字）も用語に残る長さにする。
pub const TERM_SELECTION_MAX_CHARS: usize = 30;

/// 文末の述語（「〜する」「〜した」など）で終わる選択をフレーズとみなす最小の文字数。
/// 短い語（「もったいない」など）を述語の形だけでフレーズにしないための下限。
const PREDICATE_PHRASE_MIN_CHARS: usize = 8;

/// 空白で区切った語の数がこれ以上ならフレーズとみなす（英文・語の並びの文章）。
/// 「Large Language Model」のような3語までの英語の用語は用語に残す。
const PHRASE_MIN_WORDS: usize = 4;

/// 文・節の区切りを示す記号。これを含む選択は文章（フレーズ）とみなす。
/// 半角の `!` `?` は「Yahoo!」のような名称にも使われるため含めない。
const PHRASE_PUNCTUATION: [char; 5] = ['。', '、', '，', '！', '？'];

/// 文末の述語の形。これで終わる（8字以上の）選択は文章の一部（フレーズ）とみなす。
const PREDICATE_ENDINGS: [&str; 17] = [
    "する",
    "した",
    "った",
    "される",
    "された",
    "です",
    "ます",
    "ました",
    "でした",
    "である",
    "だった",
    "ない",
    "なかった",
    "いる",
    "いた",
    "れる",
    "れた",
];

/// 選択した文字列（trim 済み・上限検証済み）が「用語」か「フレーズ（文章）」かを決める（純粋関数・決定的）。
///
/// 次のどれかに当てはまればフレーズ、どれにも当てはまらなければ用語にする（判断台帳 D21）:
/// 1. 文字数が `TERM_SELECTION_MAX_CHARS`（30字）を超える。
/// 2. 文・節の区切り記号（。、，！？）を含む。
/// 3. 空白で区切った語が `PHRASE_MIN_WORDS`（4語）以上ある。
/// 4. `PREDICATE_PHRASE_MIN_CHARS`（8字）以上で、文末の述語の形（「する」「した」「った」「です」「ない」など）で終わる。
///
/// 例外（用語のままにする）:
/// - 英字の固有名（`is_latin_proper_name`。各語が大文字・数字で始まるか短い機能語で、
///   `LATIN_PROPER_NAME_MAX_CHARS`〔60字〕以内）は、1・3 に当たっても用語にする
///   （「Federal Open Market Committee」「Organisation for Economic Co-operation and Development」）。
/// - 数字の桁区切りの「，」「、」（「１，０００億円」）は 2 の区切り記号として数えない。
///
/// 既知の限界: 名前に「。」を含む語（「モーニング娘。」）はフレーズになる。英字の見出し風の文
/// （全語が大文字始まり）は用語になる。どちらもプロンプトが変わるだけで、上限・検証・保存は同じ。
///
/// AI は使わない（同じ選択には常に同じ結果を返す）。結果は解説のプロンプト・保存する種別の選択だけに使う。
pub fn classify_selected_text(selected_text: &str) -> DictionaryEntryType {
    let text = selected_text.trim();
    if is_latin_proper_name(text) {
        return DictionaryEntryType::Term;
    }
    let char_count = text.chars().count();
    let is_phrase = char_count > TERM_SELECTION_MAX_CHARS
        || has_clause_punctuation(text)
        || text.split_whitespace().count() >= PHRASE_MIN_WORDS
        || (char_count >= PREDICATE_PHRASE_MIN_CHARS
            && PREDICATE_ENDINGS
                .iter()
                .any(|ending| text.ends_with(ending)));
    if is_phrase {
        DictionaryEntryType::Phrase
    } else {
        DictionaryEntryType::Term
    }
}

/// 英字の固有名を用語として扱う最大の文字数（「Organisation for Economic Co-operation and Development」54字が入る長さ）。
const LATIN_PROPER_NAME_MAX_CHARS: usize = 60;

/// 英字の固有名の途中に小文字で現れる短い機能語（「Bank of Japan」「Software as a Service」）。
const LATIN_NAME_FUNCTION_WORDS: [&str; 10] =
    ["of", "and", "for", "as", "a", "an", "the", "on", "in", "to"];

/// 英字の固有名（組織名・正式名称など）か。2語以上・60字以内で、どの語も英数字と `-` `&` `.` `'` だけでできており、
/// 大文字か数字で始まるか、短い機能語（`LATIN_NAME_FUNCTION_WORDS`・小文字）である。機能語だけの並びは除く。
/// 小文字の普通の語を含む英文（「The central bank raised …」）は当たらない。
fn is_latin_proper_name(text: &str) -> bool {
    if text.chars().count() > LATIN_PROPER_NAME_MAX_CHARS {
        return false;
    }
    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.len() < 2 {
        return false;
    }
    let mut has_capitalized = false;
    for word in &words {
        if !word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '&' | '.' | '\''))
        {
            return false;
        }
        let first = word.chars().next().unwrap_or(' ');
        if first.is_ascii_uppercase() || first.is_ascii_digit() {
            has_capitalized = true;
        } else if !LATIN_NAME_FUNCTION_WORDS.contains(word) {
            return false;
        }
    }
    has_capitalized
}

/// 文・節の区切り記号（`PHRASE_PUNCTUATION`）を含むか。数字に挟まれた「，」「、」（桁区切り「１，０００」）は数えない。
fn has_clause_punctuation(text: &str) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    chars.iter().enumerate().any(|(index, c)| {
        if !PHRASE_PUNCTUATION.contains(c) {
            return false;
        }
        let between_digits = matches!(c, '，' | '、')
            && index > 0
            && chars[index - 1].is_numeric()
            && chars.get(index + 1).is_some_and(|next| next.is_numeric());
        !between_digits
    })
}

/// 選択した用語が、記事の中で言葉の途中から始まる・途中で終わっていないかの判定結果
/// （`resolve_selection_boundary`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionBoundary {
    /// 言葉全体を選んでいる（または記事の文の中に見つからず判断できない）。選んだままで解説する。
    Whole,
    /// 言葉の途中だったので、前後の語境界まで広げた言葉。こちらを解説する。
    Expanded(String),
    /// 言葉の途中だが、広げ先が1つに決まらない・長すぎる。解説せずに選び直しを案内する。
    Fragment,
}

/// 選択した用語が、記事の文（画面に出ている要約・要点・抜粋など）の中で言葉の途中を切り取っていないかを調べ、
/// 途中なら前後の語境界まで広げる（純粋関数・決定的。AI は使わない）。
///
/// 2026-10-21 の実機テスト（A4）で、「フォワードデプロイドエンジニアリング」の途中から選択がずれた
/// 「ドエンジニアリング」を、ローカルAIが存在しない語としてもっともらしく解説した。小さいモデルは
/// 語の切れ端でも定義を作ってしまうため、AI を呼ぶ前に機械的に直す。
///
/// 「途中」とみなすのは、選択の最初の文字の直前（最後の文字の直後）に、同じ文字種の文字が続いているとき。
/// 文字種はカタカナ（長音「ー」・半角カナを含む）・英字・数字（いずれも全角を含む）だけを見る。
/// 英字と数字は別の文字種にする（「iPhone17」の「iPhone」、「5G対応」の「5G」を型番・数字側へ広げない）。
/// 英字は、大文字どうしの境目では広げない（「5GHz」の「5G」、「AIGC」の「AI」のような略語の並びを
/// 1語とみなさない。大文字だけの語の切れ端〔「NVIDIA」の「NVID」〕は直せないが、従来どおり解説するだけ）。
/// 漢字・ひらがなは見ない: 漢字の連なりは「日本銀行総裁」のように複数の語の複合が多く、
/// 「日本銀行」を選んだのを広げると正しい選択を壊すため。ひらがなは助詞・送り仮名と区別できない。
/// 中黒「・」や空白は語の区切りとみなす（「フォワード・デプロイド」の一部を選ぶのは正当な選択）。
///
/// - 選択が `texts` のどこかで前後とも語境界に当たって現れていれば、正しい選択とみなして `Whole`。
/// - どこにも現れない（画面の表示と保存文の違い・段落をまたいだ選択など）ときも、判断できないので `Whole`
///   （従来どおり解説する。文脈に無い語の扱いはプロンプト側の「分からないときは作らない」に任せる）。
/// - すべての出現が言葉の途中で、広げた結果が1つに決まり、用語の長さ（`TERM_SELECTION_MAX_CHARS`）に
///   収まるなら `Expanded`。広げた結果が複数ある・長すぎるなら `Fragment`。
///
/// 既知の限界: カタカナの複合語は区切りが無いので、部分語でも記事に単独で出てこなければ広げる
/// （「クラウドサービス」しか無い記事で「クラウド」を選ぶと「クラウドサービス」を解説する）。
pub fn resolve_selection_boundary(selected_text: &str, texts: &[&str]) -> SelectionBoundary {
    let selected = selected_text.trim();
    let (Some(first), Some(last)) = (selected.chars().next(), selected.chars().next_back()) else {
        return SelectionBoundary::Whole;
    };
    let mut expansions: Vec<String> = Vec::new();
    for text in texts {
        for (start, _) in text.match_indices(selected) {
            let end = start + selected.len();
            // 選択の端と外側の隣が同じ語として続いていれば（`continues_word`）、同じ文字種の間だけ広げる。
            let new_start = match text[..start].chars().next_back() {
                Some(outer) if continues_word(first, outer) => text[..start]
                    .char_indices()
                    .rev()
                    .take_while(|(_, c)| word_char_class(*c) == word_char_class(outer))
                    .last()
                    .map_or(start, |(index, _)| index),
                _ => start,
            };
            let new_end = match text[end..].chars().next() {
                Some(outer) if continues_word(last, outer) => {
                    end + text[end..]
                        .chars()
                        .take_while(|c| word_char_class(*c) == word_char_class(outer))
                        .map(char::len_utf8)
                        .sum::<usize>()
                }
                _ => end,
            };
            if new_start == start && new_end == end {
                return SelectionBoundary::Whole;
            }
            let expanded = text[new_start..new_end].to_string();
            if !expansions.contains(&expanded) {
                expansions.push(expanded);
            }
        }
    }
    match expansions.as_slice() {
        [] => SelectionBoundary::Whole,
        [expanded]
            if expanded.chars().count() <= TERM_SELECTION_MAX_CHARS
                && classify_selected_text(expanded) == DictionaryEntryType::Term =>
        {
            SelectionBoundary::Expanded(expanded.clone())
        }
        _ => SelectionBoundary::Fragment,
    }
}

/// 選択の端の文字 `inner` と外側の隣 `outer` が同じ語の続きか（`resolve_selection_boundary`）。
/// 同じ文字種で、英字なら大文字どうしでないこと。大文字どうしの判定は選択の端だけで行い、
/// いったん広げ始めたら同じ文字種の間は続ける（「ChatGPT」の「Chat」は「ChatGPT」まで広げる）。
fn continues_word(inner: char, outer: char) -> bool {
    match (word_char_class(inner), word_char_class(outer)) {
        (Some(WordCharClass::Latin), Some(WordCharClass::Latin)) => {
            !(is_upper_latin(inner) && is_upper_latin(outer))
        }
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn is_upper_latin(c: char) -> bool {
    c.is_ascii_uppercase() || ('\u{FF21}'..='\u{FF3A}').contains(&c)
}

/// 語境界の判定に使う文字種（`resolve_selection_boundary`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordCharClass {
    Katakana,
    Latin,
    Digit,
}

/// カタカナ（長音・踊り字・半角カナを含む。中黒は含まない）・英字・数字（全角を含む）だけを返す。
fn word_char_class(c: char) -> Option<WordCharClass> {
    match c {
        '\u{30A1}'..='\u{30FA}' | 'ー' | 'ヽ' | 'ヾ' | '\u{FF66}'..='\u{FF9F}' => {
            Some(WordCharClass::Katakana)
        }
        'a'..='z' | 'A'..='Z' | '\u{FF21}'..='\u{FF3A}' | '\u{FF41}'..='\u{FF5A}' => {
            Some(WordCharClass::Latin)
        }
        '0'..='9' | '\u{FF10}'..='\u{FF19}' => Some(WordCharClass::Digit),
        _ => None,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainSelectedTermParams {
    pub article_id: String,
    pub selected_text: String,
}

impl ExplainSelectedTermParams {
    pub fn validated_inputs(&self) -> Result<(String, String), AppError> {
        let article_id = self.article_id.trim();
        if article_id.is_empty() {
            return Err(AppError::Validation(
                "articleId must not be empty".to_string(),
            ));
        }

        let selected_text = self.selected_text.trim();
        if selected_text.is_empty() {
            return Err(AppError::Validation(
                "selectedText must not be empty".to_string(),
            ));
        }

        // trim 後の文字数上限。超過時は入力本文をエラー・ログへ含めず固定文言で拒否する
        // （この後段の記事取得・辞書検索・AI呼び出し・辞書保存には一切進まない）。
        if selected_text.chars().count() > TERM_EXPLANATION_SELECTED_TEXT_MAX_CHARS {
            return Err(AppError::Validation(format!(
                "selectedText must be {TERM_EXPLANATION_SELECTED_TEXT_MAX_CHARS} characters or fewer"
            )));
        }

        Ok((article_id.to_string(), selected_text.to_string()))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDictionaryEntriesParams {
    pub keyword: Option<String>,
    #[serde(rename = "type")]
    pub entry_type: Option<String>,
    pub starred_only: Option<bool>,
}

impl ListDictionaryEntriesParams {
    pub fn validated_filters(
        &self,
    ) -> Result<(Option<String>, Option<DictionaryEntryType>, bool), AppError> {
        let keyword = self
            .keyword
            .as_ref()
            .map(|value| normalize_text(value))
            .filter(|value| !value.is_empty());
        let entry_type = self
            .entry_type
            .as_deref()
            .map(DictionaryEntryType::from_filter_value)
            .transpose()?;
        let starred_only = self.starred_only.unwrap_or(false);

        Ok((keyword, entry_type, starred_only))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDictionaryEntryParams {
    pub entry: DictionaryEntryDto,
}

impl SaveDictionaryEntryParams {
    pub fn validated_entry(&self) -> Result<DictionaryEntryDto, AppError> {
        let mut entry = self.entry.clone();
        entry.entry_id = entry.entry_id.trim().to_string();
        entry.key_text = entry.key_text.trim().to_string();
        entry.short_explanation = entry.short_explanation.trim().to_string();
        entry.detail_explanation = entry.detail_explanation.trim().to_string();
        entry.related_article_id = entry
            .related_article_id
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        entry.related_article_title = entry
            .related_article_title
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        entry.validate()?;
        Ok(entry)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDictionaryMemoParams {
    pub entry_id: String,
    pub memo: String,
}

impl UpdateDictionaryMemoParams {
    /// entryId を検証し、memo を整形して返す（空文字はメモ削除＝None）。
    pub fn validated(&self) -> Result<(String, Option<String>), AppError> {
        let entry_id = self.entry_id.trim();
        if entry_id.is_empty() {
            return Err(AppError::Validation(
                "entryId must not be empty".to_string(),
            ));
        }

        // 改行コードは \n に統一する。画面の textarea は既に \n だが、command 直接呼び出しの \r\n / \r も
        // 同じ改行として受け入れ、保存データに \r を残さない。
        let normalized = self.memo.replace("\r\n", "\n").replace('\r', "\n");
        let memo = normalized.trim();
        if memo.chars().count() > 1000 {
            return Err(AppError::Validation(
                "memo must be 1000 characters or fewer".to_string(),
            ));
        }

        // メモは複数行の自由記述なので改行は許可する。タブも表示上は空白と同じで無害なうえ、
        // 表計算などからの貼り付けに含まれやすいため許可する。それ以外の制御文字（ESC・NUL など）は拒否する
        // （セキュリティ詳細設計書 §15.2）。
        if contains_disallowed_control_char(memo) {
            return Err(AppError::Validation(
                "memo must not contain control characters".to_string(),
            ));
        }

        let memo = if memo.is_empty() {
            None
        } else {
            Some(memo.to_string())
        };
        Ok((entry_id.to_string(), memo))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDictionaryFavoriteParams {
    pub entry_id: String,
    pub is_starred: bool,
}

impl UpdateDictionaryFavoriteParams {
    pub fn validated(&self) -> Result<(String, bool), AppError> {
        let entry_id = self.entry_id.trim();
        if entry_id.is_empty() {
            return Err(AppError::Validation(
                "entryId must not be empty".to_string(),
            ));
        }
        Ok((entry_id.to_string(), self.is_starred))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDictionaryEntryParams {
    pub entry_id: String,
}

impl DeleteDictionaryEntryParams {
    pub fn validated_entry_id(&self) -> Result<String, AppError> {
        let entry_id = self.entry_id.trim();
        if entry_id.is_empty() {
            return Err(AppError::Validation(
                "entryId must not be empty".to_string(),
            ));
        }
        Ok(entry_id.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PersistedDictionaryStore {
    pub version: u32,
    pub entries: Vec<PersistedDictionaryEntry>,
}

impl PersistedDictionaryStore {
    pub fn with_current_version() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedDictionaryEntry {
    pub version: u32,
    pub dictionary_id: String,
    pub target_text: String,
    pub normalized_text: String,
    // 種別を持たない旧データは「用語」として読む（辞書全体の読み込みを失敗させない・D21）。
    #[serde(default)]
    pub entry_type: DictionaryEntryType,
    pub short_explanation: String,
    pub detail_explanation: String,
    // 旧データに無い場合でも辞書全体の読み込みを失敗させないため既定値で補う（空文字 / 0）。
    #[serde(default)]
    pub created_at: String,
    pub last_referenced_at: Option<String>,
    #[serde(default)]
    pub reference_count: u32,
    pub source_article_ids: Vec<String>,
    pub favorite: bool,
    pub memo: Option<String>,
    pub related_article_title: Option<String>,
    pub related_article_id: Option<String>,
}

impl PersistedDictionaryEntry {
    pub fn from_dto(entry: DictionaryEntryDto, now_text: String) -> Self {
        let normalized_text = entry.normalized_key_text();
        let source_article_ids = entry
            .related_article_id
            .clone()
            .into_iter()
            .collect::<Vec<_>>();

        Self {
            version: 1,
            dictionary_id: entry.entry_id,
            target_text: entry.key_text,
            normalized_text,
            entry_type: entry.entry_type,
            short_explanation: entry.short_explanation,
            detail_explanation: entry.detail_explanation,
            created_at: now_text.clone(),
            last_referenced_at: Some(now_text),
            reference_count: 1,
            source_article_ids,
            // 辞書保存は ★ を付けない。★ は辞書画面などの ★ 操作（update_dictionary_favorite）だけで変える
            // （要件定義書 §7.4.10）。保存パラメータの isStarred は参照しない。
            favorite: false,
            memo: None,
            related_article_title: entry.related_article_title,
            related_article_id: entry.related_article_id,
        }
    }

    pub fn apply_from_dto(&mut self, entry: DictionaryEntryDto, now_text: String) {
        self.target_text = entry.key_text;
        self.normalized_text = normalize_text(&self.target_text);
        self.entry_type = entry.entry_type;
        self.short_explanation = entry.short_explanation;
        self.detail_explanation = entry.detail_explanation;
        // 再保存でも ★ は変えない（★ 操作は update_dictionary_favorite だけ・要件定義書 §7.4.10）。
        self.related_article_title = entry.related_article_title;
        self.related_article_id = entry.related_article_id.clone();
        self.last_referenced_at = Some(now_text);
        self.reference_count = self.reference_count.saturating_add(1);

        if let Some(article_id) = entry.related_article_id {
            let already_present = self
                .source_article_ids
                .iter()
                .any(|value| value == &article_id);
            if !already_present {
                self.source_article_ids.push(article_id);
            }
        }
    }

    pub fn to_dto(&self) -> DictionaryEntryDto {
        DictionaryEntryDto {
            entry_id: self.dictionary_id.clone(),
            key_text: self.target_text.clone(),
            entry_type: self.entry_type.clone(),
            short_explanation: self.short_explanation.clone(),
            detail_explanation: self.detail_explanation.clone(),
            related_article_id: self.related_article_id.clone(),
            related_article_title: self.related_article_title.clone(),
            is_starred: self.favorite,
            // 永続化済みの項目から作る DTO なので常に「辞書保存済み」。
            saved_in_dictionary: true,
            is_notice: false,
            expanded_from: None,
        }
    }

    pub fn to_list_item_dto(&self) -> DictionaryEntryListItemDto {
        DictionaryEntryListItemDto {
            entry_id: self.dictionary_id.clone(),
            key_text: self.target_text.clone(),
            entry_type: self.entry_type.clone(),
            short_explanation: self.short_explanation.clone(),
            detail_explanation: self.detail_explanation.clone(),
            related_article_id: self.related_article_id.clone(),
            related_article_title: self.related_article_title.clone(),
            last_viewed_at_text: self
                .last_referenced_at
                .clone()
                .or_else(|| Some(self.created_at.clone())),
            created_at_text: Some(self.created_at.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            reference_count: self.reference_count,
            memo: self.memo.clone(),
            is_starred: self.favorite,
        }
    }
}

pub fn normalize_text(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        classify_selected_text, resolve_selection_boundary, DeleteDictionaryEntryParams,
        DictionaryEntryDto, DictionaryEntryType, ExplainSelectedTermParams,
        ListDictionaryEntriesParams, PersistedDictionaryEntry, SaveDictionaryEntryParams,
        SelectionBoundary, UpdateDictionaryFavoriteParams, UpdateDictionaryMemoParams,
    };

    #[test]
    fn validated_inputs_trim_values() {
        let params = ExplainSelectedTermParams {
            article_id: " article-001 ".to_string(),
            selected_text: "  生成AI  ".to_string(),
        };

        let (article_id, selected_text) = params.validated_inputs().unwrap();
        assert_eq!(article_id, "article-001");
        assert_eq!(selected_text, "生成AI");
    }

    #[test]
    fn validated_inputs_reject_empty_values() {
        let params = ExplainSelectedTermParams {
            article_id: " ".to_string(),
            selected_text: " ".to_string(),
        };

        assert!(params.validated_inputs().is_err());
    }

    // selectedText の文字数上限（200文字・trim 後・chars 単位）。
    fn explain_params(selected_text: &str) -> ExplainSelectedTermParams {
        ExplainSelectedTermParams {
            article_id: "article-001".to_string(),
            selected_text: selected_text.to_string(),
        }
    }

    #[test]
    fn validated_inputs_accepts_up_to_200_chars() {
        // 199文字・200文字ちょうどは許可。
        assert!(explain_params(&"a".repeat(199)).validated_inputs().is_ok());
        let ok = explain_params(&"a".repeat(200)).validated_inputs().unwrap();
        assert_eq!(ok.1.chars().count(), 200);
    }

    #[test]
    fn validated_inputs_rejects_201_chars_without_leaking_body() {
        let body = "a".repeat(201);
        let err = explain_params(&body).validated_inputs().unwrap_err();
        // 入力本文（201文字分）はエラー文へ含めない。文字数上限(200)の固定文言のみ。
        let message = err.to_string();
        assert!(!message.contains(&body));
        assert!(!message.contains("aaaa"));
        assert!(message.contains("200"));
    }

    #[test]
    fn validated_inputs_counts_multibyte_by_chars_not_bytes() {
        // 日本語200文字（600バイト）は許可、201文字は拒否（chars 判定）。
        assert!(explain_params(&"あ".repeat(200)).validated_inputs().is_ok());
        assert!(explain_params(&"あ".repeat(201))
            .validated_inputs()
            .is_err());
        // 絵文字（サロゲートペア相当・4バイト）でも文字数で判定する。
        assert!(explain_params(&"😀".repeat(200)).validated_inputs().is_ok());
        assert!(explain_params(&"😀".repeat(201))
            .validated_inputs()
            .is_err());
    }

    #[test]
    fn validated_inputs_counts_after_trim() {
        // 前後空白を trim した後の文字数で判定する。200文字＋前後空白 → 許可。
        let padded = format!("  {}  ", "あ".repeat(200));
        let ok = explain_params(&padded).validated_inputs().unwrap();
        assert_eq!(ok.1.chars().count(), 200);
        // trim 後に 201 文字なら拒否。
        let padded_over = format!("  {}  ", "あ".repeat(201));
        assert!(explain_params(&padded_over).validated_inputs().is_err());
    }

    #[test]
    fn validated_filters_normalize_keyword_and_type() {
        let params = ListDictionaryEntriesParams {
            keyword: Some("  生成AI ".to_string()),
            entry_type: Some("term".to_string()),
            starred_only: Some(true),
        };

        let (keyword, entry_type, starred_only) = params.validated_filters().unwrap();
        assert_eq!(keyword.as_deref(), Some("生成ai"));
        assert_eq!(entry_type, Some(DictionaryEntryType::Term));
        assert!(starred_only);
    }

    #[test]
    fn validated_filters_reject_unknown_type() {
        let params = ListDictionaryEntriesParams {
            keyword: None,
            entry_type: Some("unknown".to_string()),
            starred_only: None,
        };

        assert!(params.validated_filters().is_err());
    }

    #[test]
    fn validated_entry_trims_and_accepts_valid_payload() {
        let params = SaveDictionaryEntryParams {
            entry: DictionaryEntryDto {
                entry_id: " entry-1 ".to_string(),
                key_text: " 生成AI ".to_string(),
                entry_type: DictionaryEntryType::Term,
                short_explanation: " 短い説明 ".to_string(),
                detail_explanation: " 詳細説明 ".to_string(),
                related_article_id: Some(" article-001 ".to_string()),
                related_article_title: Some(" 記事タイトル ".to_string()),
                is_starred: true,
                saved_in_dictionary: false,
                is_notice: false,
                expanded_from: None,
            },
        };

        let entry = params.validated_entry().unwrap();
        assert_eq!(entry.entry_id, "entry-1");
        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.short_explanation, "短い説明");
        assert_eq!(entry.detail_explanation, "詳細説明");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
        assert_eq!(entry.related_article_title.as_deref(), Some("記事タイトル"));
    }

    #[test]
    fn validated_entry_rejects_missing_required_fields() {
        let params = SaveDictionaryEntryParams {
            entry: DictionaryEntryDto {
                entry_id: String::new(),
                key_text: " ".to_string(),
                entry_type: DictionaryEntryType::Term,
                short_explanation: " ".to_string(),
                detail_explanation: " ".to_string(),
                related_article_id: None,
                related_article_title: None,
                is_starred: false,
                saved_in_dictionary: false,
                is_notice: false,
                expanded_from: None,
            },
        };

        assert!(params.validated_entry().is_err());
    }

    #[test]
    fn update_memo_params_validate_and_trim() {
        let params = UpdateDictionaryMemoParams {
            entry_id: " entry-1 ".to_string(),
            memo: "  あとで読む  ".to_string(),
        };
        let (entry_id, memo) = params.validated().unwrap();
        assert_eq!(entry_id, "entry-1");
        assert_eq!(memo.as_deref(), Some("あとで読む"));
    }

    #[test]
    fn update_memo_params_empty_memo_clears() {
        let params = UpdateDictionaryMemoParams {
            entry_id: "entry-1".to_string(),
            memo: "   ".to_string(),
        };
        let (_, memo) = params.validated().unwrap();
        assert!(memo.is_none());
    }

    #[test]
    fn update_memo_params_reject_empty_entry_id() {
        let params = UpdateDictionaryMemoParams {
            entry_id: " ".to_string(),
            memo: "x".to_string(),
        };
        assert!(params.validated().is_err());
    }

    #[test]
    fn update_memo_params_allow_newline_and_tab() {
        let params = UpdateDictionaryMemoParams {
            entry_id: "entry-1".to_string(),
            memo: "一行目\n\t二行目".to_string(),
        };
        let (_, memo) = params.validated().unwrap();
        assert_eq!(memo.as_deref(), Some("一行目\n\t二行目"));
    }

    #[test]
    fn update_memo_params_normalize_crlf_and_cr_to_lf() {
        let params = UpdateDictionaryMemoParams {
            entry_id: "entry-1".to_string(),
            memo: "一行目\r\n二行目\r三行目\r\n".to_string(),
        };
        let (_, memo) = params.validated().unwrap();
        assert_eq!(memo.as_deref(), Some("一行目\n二行目\n三行目"));
    }

    #[test]
    fn update_memo_params_reject_other_control_chars_without_leaking_body() {
        for memo in ["秘密\u{1b}[31m", "秘密\u{0}", "秘密\u{7f}", "秘密\u{8}"] {
            let params = UpdateDictionaryMemoParams {
                entry_id: "entry-1".to_string(),
                memo: memo.to_string(),
            };
            match params.validated() {
                Err(crate::error::AppError::Validation(message)) => {
                    assert_eq!(message, "memo must not contain control characters");
                    assert!(!message.contains("秘密"));
                }
                other => panic!("expected validation error, got {other:?}"),
            }
        }
    }

    #[test]
    fn update_memo_params_count_crlf_as_one_char() {
        // \r\n を \n に揃えてから数えるので、Windows 改行のメモが上限で不当に弾かれない。
        let memo = format!("{}\r\n{}", "あ".repeat(499), "い".repeat(500));
        let params = UpdateDictionaryMemoParams {
            entry_id: "entry-1".to_string(),
            memo,
        };
        assert!(params.validated().is_ok());
    }

    #[test]
    fn update_favorite_params_validate_and_trim() {
        let params = UpdateDictionaryFavoriteParams {
            entry_id: " entry-1 ".to_string(),
            is_starred: true,
        };
        let (entry_id, is_starred) = params.validated().unwrap();
        assert_eq!(entry_id, "entry-1");
        assert!(is_starred);
    }

    #[test]
    fn update_favorite_params_reject_empty_entry_id() {
        let params = UpdateDictionaryFavoriteParams {
            entry_id: " ".to_string(),
            is_starred: true,
        };
        assert!(params.validated().is_err());
    }

    #[test]
    fn delete_params_validate_and_reject_empty() {
        assert_eq!(
            DeleteDictionaryEntryParams {
                entry_id: " entry-1 ".to_string(),
            }
            .validated_entry_id()
            .unwrap(),
            "entry-1"
        );
        assert!(DeleteDictionaryEntryParams {
            entry_id: " ".to_string(),
        }
        .validated_entry_id()
        .is_err());
    }

    fn persisted_entry_json() -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "dictionaryId": "entry-1",
            "targetText": "生成AI",
            "normalizedText": "生成ai",
            "entryType": "term",
            "shortExplanation": "短い説明",
            "detailExplanation": "詳しい説明",
            "createdAt": "1779246000",
            "lastReferencedAt": "1779332400",
            "referenceCount": 3,
            "sourceArticleIds": ["article-1"],
            "favorite": false,
            "memo": null,
            "relatedArticleTitle": null,
            "relatedArticleId": null
        })
    }

    #[test]
    fn list_item_dto_includes_created_at_and_reference_count() {
        let entry: PersistedDictionaryEntry =
            serde_json::from_value(persisted_entry_json()).unwrap();
        let dto = entry.to_list_item_dto();
        assert_eq!(dto.created_at_text.as_deref(), Some("1779246000"));
        assert_eq!(dto.reference_count, 3);

        let value = serde_json::to_value(&dto).unwrap();
        assert_eq!(value["createdAtText"], "1779246000");
        assert_eq!(value["referenceCount"], 3);
    }

    #[test]
    fn legacy_entry_without_created_at_and_reference_count_still_maps() {
        // 作成日時・参照回数を持たない旧データでも読み込めて、作成日時なし・参照回数0として返す。
        let mut json = persisted_entry_json();
        let object = json.as_object_mut().unwrap();
        object.remove("createdAt");
        object.remove("referenceCount");

        let entry: PersistedDictionaryEntry = serde_json::from_value(json).unwrap();
        let dto = entry.to_list_item_dto();
        assert_eq!(dto.created_at_text, None);
        assert_eq!(dto.reference_count, 0);
        assert_eq!(dto.last_viewed_at_text.as_deref(), Some("1779332400"));

        let value = serde_json::to_value(&dto).unwrap();
        assert!(value["createdAtText"].is_null());
        assert_eq!(value["referenceCount"], 0);
    }

    #[test]
    fn blank_created_at_maps_to_none() {
        let mut json = persisted_entry_json();
        json["createdAt"] = serde_json::json!("  ");
        let entry: PersistedDictionaryEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.to_list_item_dto().created_at_text, None);
    }

    #[test]
    fn unknown_type_filter_message_excludes_input_value() {
        match DictionaryEntryType::from_filter_value("secret-type<script>") {
            Err(crate::error::AppError::Validation(message)) => {
                assert_eq!(message, "type must be one of term, phrase");
                assert!(!message.contains("secret-type"));
            }
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    // --- 用語 / フレーズの判定（D21） ---

    #[test]
    fn classify_keeps_short_words_and_names_as_terms() {
        for term in [
            "生成AI",
            "SaaS",
            "資金調達",
            "もったいない",
            "Yahoo!",
            "Large Language Model",
            "日本銀行 金融政策決定会合",
            // 正式名称の長い用語（27字）も用語のまま。
            "環太平洋パートナーシップに関する包括的及び先進的な協定",
        ] {
            assert_eq!(
                classify_selected_text(term),
                DictionaryEntryType::Term,
                "{term}"
            );
        }
    }

    #[test]
    fn classify_treats_sentences_and_clauses_as_phrases() {
        for phrase in [
            // 句点・読点・全角の感嘆符・疑問符を含む。
            "物価は上がった。",
            "円安が進み、輸入品の値段が上がった",
            "本当に必要なのか？",
            // 文末の述語で終わる8字以上の選択。
            "政府は補助金を拡充する",
            "日銀は利上げを見送った",
            "需要は回復していない",
            // 4語以上の英文。
            "The central bank raised rates",
        ] {
            assert_eq!(
                classify_selected_text(phrase),
                DictionaryEntryType::Phrase,
                "{phrase}"
            );
        }
    }

    #[test]
    fn classify_uses_the_term_length_boundary() {
        // 30字ちょうどは用語、31字はフレーズ（記号・述語の形を含まない文字列で境界だけを見る）。
        assert_eq!(
            classify_selected_text(&"ア".repeat(super::TERM_SELECTION_MAX_CHARS)),
            DictionaryEntryType::Term
        );
        assert_eq!(
            classify_selected_text(&"ア".repeat(super::TERM_SELECTION_MAX_CHARS + 1)),
            DictionaryEntryType::Phrase
        );
        // 前後の空白は数えない。
        assert_eq!(
            classify_selected_text(&format!("  {}  ", "ア".repeat(30))),
            DictionaryEntryType::Term
        );
        // 述語の形でも8字未満は用語のまま（「する」で終わる短い語）。
        assert_eq!(
            classify_selected_text("炎上する"),
            DictionaryEntryType::Term
        );
    }

    #[test]
    fn classify_keeps_latin_proper_names_as_terms() {
        for name in [
            "Federal Open Market Committee",
            "Software as a Service",
            // 30字を超える正式名称（54字）も、固有名なら用語のまま。
            "Organisation for Economic Co-operation and Development",
            "Bank of Japan Monetary Policy Meeting",
            "AT&T Inc.",
            "Windows 11 Pro for Workstations",
        ] {
            assert_eq!(
                classify_selected_text(name),
                DictionaryEntryType::Term,
                "{name}"
            );
        }
    }

    #[test]
    fn classify_keeps_english_sentences_as_phrases() {
        for sentence in [
            "The central bank raised interest rates again this week",
            "The central bank raised rates",
            // 機能語だけの並びは固有名とみなさない。
            "of the and for",
            // 固有名の形でも上限（60字）を超えたら固有名とみなさない。
            "Very Long Title Case Words That Keep Going Beyond Sixty Characters Total",
        ] {
            assert_eq!(
                classify_selected_text(sentence),
                DictionaryEntryType::Phrase,
                "{sentence}"
            );
        }
    }

    #[test]
    fn classify_ignores_digit_group_separators() {
        // 数字に挟まれた「，」「、」は桁区切りとして数えない。
        assert_eq!(
            classify_selected_text("１，０００億円"),
            DictionaryEntryType::Term
        );
        assert_eq!(
            classify_selected_text("1，000億円"),
            DictionaryEntryType::Term
        );
        // 数字に挟まれていない読点は従来どおりフレーズの手がかり。
        assert_eq!(
            classify_selected_text("1000億円、増加"),
            DictionaryEntryType::Phrase
        );
    }

    #[test]
    fn classify_known_limitation_name_with_period() {
        // 既知の限界: 名前に「。」を含む語はフレーズになる（プロンプトが変わるだけで上限・保存は同じ）。
        assert_eq!(
            classify_selected_text("モーニング娘。"),
            DictionaryEntryType::Phrase
        );
    }

    #[test]
    fn classify_is_deterministic() {
        let text = "円安が進み、輸入品の値段が上がった";
        assert_eq!(classify_selected_text(text), classify_selected_text(text));
    }

    #[test]
    fn legacy_entry_without_entry_type_loads_as_term() {
        // 種別を持たない旧データは「用語」として読む（辞書全体の読み込みを失敗させない）。
        let mut json = persisted_entry_json();
        json.as_object_mut().unwrap().remove("entryType");
        let entry: PersistedDictionaryEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.entry_type, DictionaryEntryType::Term);
        assert_eq!(
            entry.to_list_item_dto().entry_type,
            DictionaryEntryType::Term
        );
    }

    #[test]
    fn phrase_entry_type_round_trips_through_storage_and_dto() {
        let mut json = persisted_entry_json();
        json["entryType"] = serde_json::json!("phrase");
        let entry: PersistedDictionaryEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.entry_type, DictionaryEntryType::Phrase);
        let value = serde_json::to_value(entry.to_list_item_dto()).unwrap();
        assert_eq!(value["type"], "phrase");
        let stored = serde_json::to_value(&entry).unwrap();
        assert_eq!(stored["entryType"], "phrase");
    }

    #[test]
    fn legacy_key_point_entry_loads_and_saves_as_phrase() {
        // 旧種別 key_point（要点説明）は phrase に統合済み（D144）。読み込みを失敗させずフレーズとして扱い、
        // 次に保存されるときは phrase で書き出す。
        let mut json = persisted_entry_json();
        json["entryType"] = serde_json::json!("key_point");
        let entry: PersistedDictionaryEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.entry_type, DictionaryEntryType::Phrase);
        let value = serde_json::to_value(entry.to_list_item_dto()).unwrap();
        assert_eq!(value["type"], "phrase");
        let stored = serde_json::to_value(&entry).unwrap();
        assert_eq!(stored["entryType"], "phrase");
    }

    #[test]
    fn legacy_key_point_filter_value_maps_to_phrase() {
        assert_eq!(
            DictionaryEntryType::from_filter_value("key_point").unwrap(),
            DictionaryEntryType::Phrase
        );
    }

    #[test]
    fn save_params_without_type_default_to_term() {
        // 種別を送らない旧画面からの保存も受け付け、用語として保存する。
        let params: SaveDictionaryEntryParams = serde_json::from_value(serde_json::json!({
            "entry": {
                "entryId": "entry-1",
                "keyText": "生成AI",
                "shortExplanation": "短い説明",
                "detailExplanation": "詳しい説明",
                "isStarred": false
            }
        }))
        .unwrap();
        assert_eq!(
            params.validated_entry().unwrap().entry_type,
            DictionaryEntryType::Term
        );
    }

    // --- 選択の語境界（resolve_selection_boundary） ---

    const FDE_SUMMARY: &str =
        "米企業がFDE（フォワードデプロイドエンジニアリング）の担当者を増やしている。";

    fn expanded(word: &str) -> SelectionBoundary {
        SelectionBoundary::Expanded(word.to_string())
    }

    #[test]
    fn boundary_extends_a_katakana_selection_cut_in_the_middle() {
        // 実機テスト A4 の例: 「フォワードデプロイド」の途中から選択がずれた。
        assert_eq!(
            resolve_selection_boundary("ドエンジニアリング", &[FDE_SUMMARY]),
            expanded("フォワードデプロイドエンジニアリング")
        );
        // 終わりが途中で切れた選択も広げる。
        assert_eq!(
            resolve_selection_boundary("フォワードデプロイ", &[FDE_SUMMARY]),
            expanded("フォワードデプロイドエンジニアリング")
        );
        // 両側とも途中。
        assert_eq!(
            resolve_selection_boundary("ワードデプ", &[FDE_SUMMARY]),
            expanded("フォワードデプロイドエンジニアリング")
        );
    }

    #[test]
    fn boundary_extends_latin_words_cut_in_the_middle() {
        let text = "OpenAIはChatGPTの新版を出した。";
        assert_eq!(
            resolve_selection_boundary("hatGPT", &[text]),
            expanded("ChatGPT")
        );
        assert_eq!(
            resolve_selection_boundary("Ｃｈａｔ", &["新しいＣｈａｔＧＰＴの話"]),
            expanded("ＣｈａｔＧＰＴ")
        );
    }

    #[test]
    fn boundary_keeps_whole_words_unchanged() {
        for (selected, text) in [
            ("フォワードデプロイドエンジニアリング", FDE_SUMMARY),
            ("FDE", FDE_SUMMARY),
            ("担当者", FDE_SUMMARY),
            // 文字種が変わるところは語境界（英字→カタカナ、カタカナ→漢字・ひらがな）。
            ("AI", "生成AIエージェントを使う"),
            ("エージェント", "生成AIエージェントを使う"),
            ("データ", "データを集める"),
            // 中黒・空白は語の区切り。
            ("デプロイド", "フォワード・デプロイド・エンジニア"),
            ("Bank", "Bank of Japan"),
        ] {
            assert_eq!(
                resolve_selection_boundary(selected, &[text]),
                SelectionBoundary::Whole,
                "{selected}"
            );
        }
    }

    #[test]
    fn boundary_does_not_extend_latin_into_digits_or_between_capitals() {
        // 英字と数字は別の文字種（型番へ広げない）。
        for (selected, text) in [
            ("iPhone", "新しいiPhone17を発表した"),
            ("17", "新しいiPhone17を発表した"),
            ("5G", "5G対応の端末"),
            // 大文字どうしの境目は略語の並びなので広げない。
            ("5G", "5GHz帯を使う"),
            ("AI", "AIGCの市場"),
        ] {
            assert_eq!(
                resolve_selection_boundary(selected, &[text]),
                SelectionBoundary::Whole,
                "{selected} / {text}"
            );
        }
        // 小文字をはさむ切れ端は広げる（大文字→小文字・小文字→大文字の境目は語の中）。
        assert_eq!(
            resolve_selection_boundary("Open", &["OpenAIが発表"]),
            expanded("OpenAI")
        );
        // 大文字どうしの判定は選択の端だけ。広げ始めたら英字の間は続ける。
        assert_eq!(
            resolve_selection_boundary("Chat", &["ChatGPTの新版"]),
            expanded("ChatGPT")
        );
        assert_eq!(
            resolve_selection_boundary("Phone", &["新しいiPhone17を発表した"]),
            expanded("iPhone")
        );
        assert_eq!(
            resolve_selection_boundary("02", &["2026年"]),
            expanded("2026")
        );
    }

    #[test]
    fn boundary_extends_a_katakana_sub_word_that_never_stands_alone_known_limit() {
        // 既知の限界（設計書 §5.3.4）: カタカナ複合語の部分語は、単独で出てこなければ複合語へ広げる。
        assert_eq!(
            resolve_selection_boundary("クラウド", &["クラウドサービスの利用が増えた"]),
            expanded("クラウドサービス")
        );
        // 単独でも出ていれば広げない。
        assert_eq!(
            resolve_selection_boundary(
                "クラウド",
                &["クラウドサービスの利用が増えた", "クラウドに移す"]
            ),
            SelectionBoundary::Whole
        );
    }

    #[test]
    fn boundary_does_not_extend_kanji_or_hiragana_runs() {
        // 漢字の連なりは複合語が多いので広げない（「日本銀行」は正しい選択）。
        assert_eq!(
            resolve_selection_boundary("日本銀行", &["日本銀行総裁が会見した"]),
            SelectionBoundary::Whole
        );
        assert_eq!(
            resolve_selection_boundary("務省", &["外務省が発表した"]),
            SelectionBoundary::Whole
        );
        assert_eq!(
            resolve_selection_boundary("かりやすい", &["わかりやすい説明"]),
            SelectionBoundary::Whole
        );
    }

    #[test]
    fn boundary_prefers_whole_when_any_occurrence_is_standalone() {
        // 1か所でも言葉全体として出ていれば、選んだままにする（部分語の正しい選択を壊さない）。
        let texts = ["データセンターを増やす", "データの量が多い"];
        assert_eq!(
            resolve_selection_boundary("データ", &texts),
            SelectionBoundary::Whole
        );
        // 言葉全体として出ていなければ、複合語へ広げる。
        assert_eq!(
            resolve_selection_boundary("センター", &texts),
            expanded("データセンター")
        );
    }

    #[test]
    fn boundary_is_whole_when_the_selection_is_not_found() {
        assert_eq!(
            resolve_selection_boundary("ドエンジニアリング", &["別の記事の文"]),
            SelectionBoundary::Whole
        );
        assert_eq!(
            resolve_selection_boundary("ドエンジニアリング", &[]),
            SelectionBoundary::Whole
        );
        assert_eq!(
            resolve_selection_boundary("  ", &[FDE_SUMMARY]),
            SelectionBoundary::Whole
        );
    }

    #[test]
    fn boundary_is_fragment_when_extensions_disagree_or_are_too_long() {
        // 広げ先が2つに分かれる。
        assert_eq!(
            resolve_selection_boundary("ンター", &["データセンターとコールセンター"]),
            SelectionBoundary::Fragment
        );
        // 広げると用語の長さ（30字）を超える。
        let long = format!("ア{}イ", "ウ".repeat(40));
        assert_eq!(
            resolve_selection_boundary("ウウウ", &[long.as_str()]),
            SelectionBoundary::Fragment
        );
    }

    #[test]
    fn entry_dto_new_fields_are_compatible_with_old_payloads() {
        // 旧画面の保存要求（isNotice / expandedFrom なし）も受け取れる。
        let old: DictionaryEntryDto = serde_json::from_value(serde_json::json!({
            "entryId": "entry-1",
            "keyText": "生成AI",
            "type": "term",
            "shortExplanation": "短い説明",
            "detailExplanation": "詳しい説明",
            "isStarred": false
        }))
        .unwrap();
        assert!(!old.is_notice);
        assert_eq!(old.expanded_from, None);

        // 返すときは isNotice を常に含め、expandedFrom は広げたときだけ含める。
        let value = serde_json::to_value(&old).unwrap();
        assert_eq!(value["isNotice"], false);
        assert!(value.get("expandedFrom").is_none());
        let expanded = DictionaryEntryDto {
            expanded_from: Some("成AI".to_string()),
            ..old
        };
        assert_eq!(
            serde_json::to_value(&expanded).unwrap()["expandedFrom"],
            "成AI"
        );
    }
}
