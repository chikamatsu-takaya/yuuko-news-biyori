//! テーマの地に敷く模様のドメイン（判断台帳 D152〜D154）。
//!
//! - 模様は色テーマとは独立に選ぶ（色テーマ × 模様）。保存先は設定 `ui.patternId`。
//! - 選べるのは「なし」・初期所持の模様（水玉・肉球）・ガチャで獲得した模様（ガチャマスタの `Pattern` 種別）。
//! - 既定は新規・既存ユーザーとも「なし」（今の見た目を変えない）。未知・未所持の ID は適用時に「なし」へ倒す。
//!
//! 模様の形（SVG）と濃さ・大きさはフロント（lib/ui-pattern.mjs・public/assets/patterns）が持ち、
//! ここは「どの ID を選んでよいか」だけを決める（テーマの `domain/reward.rs` と同じ分担）。

use serde::Serialize;

use crate::domain::gacha::{find_gacha_item, GachaItemKind, GACHA_MASTER};

/// 模様なし（既定）。lib/ui-pattern.mjs の PATTERN_NONE_ID と一致させる。
pub const PATTERN_NONE_ID: &str = "none";

/// 最初から使える模様（「なし」を除く）。ガチャには入れない。lib/ui-pattern.mjs の UI_PATTERNS と一致させる。
pub const INITIAL_PATTERN_IDS: &[&str] = &["pattern_dots", "pattern_paw"];

/// ガチャマスタにある模様か。
pub fn is_gacha_pattern(pattern_id: &str) -> bool {
    find_gacha_item(pattern_id).is_some_and(|def| def.kind == GachaItemKind::Pattern)
}

/// 利用者が選んでよい模様か。「なし」・初期所持・所持済みのガチャ模様だけを許す
/// （React から任意の ID を書き込ませないため、判定は Rust に置く）。
/// `owned_gacha_pattern_ids` はガチャ状態の所持 ID。マスタにある模様種別の ID だけを有効とみなす。
pub fn is_selectable_pattern(pattern_id: &str, owned_gacha_pattern_ids: &[String]) -> bool {
    if pattern_id == PATTERN_NONE_ID || INITIAL_PATTERN_IDS.contains(&pattern_id) {
        return true;
    }
    is_gacha_pattern(pattern_id) && owned_gacha_pattern_ids.iter().any(|id| id == pattern_id)
}

/// 選べる模様の ID 一覧（「なし」→ 初期所持 → ガチャマスタ順の所持済み）。
pub fn selectable_pattern_ids(owned_gacha_pattern_ids: &[String]) -> Vec<String> {
    let mut ids: Vec<String> = std::iter::once(PATTERN_NONE_ID)
        .chain(INITIAL_PATTERN_IDS.iter().copied())
        .map(str::to_string)
        .collect();
    ids.extend(
        GACHA_MASTER
            .iter()
            .filter(|def| def.kind == GachaItemKind::Pattern)
            .filter(|def| owned_gacha_pattern_ids.iter().any(|id| id == def.item_id))
            .map(|def| def.item_id.to_string()),
    );
    ids
}

/// 保存済みの模様 ID を、適用してよい ID へ解決する。前後の空白は除き、選べない ID は「なし」へ倒す。
pub fn effective_pattern_id(saved_pattern_id: &str, owned_gacha_pattern_ids: &[String]) -> String {
    let trimmed = saved_pattern_id.trim();
    if is_selectable_pattern(trimmed, owned_gacha_pattern_ids) {
        trimmed.to_string()
    } else {
        PATTERN_NONE_ID.to_string()
    }
}

/// `get_pattern_state` / `set_active_pattern` の戻り値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternStateDto {
    /// 適用中の模様 ID（選べない・未知の ID は "none"）。
    pub active_pattern_id: String,
    /// いま選べる模様の ID（"none"・初期所持・所持済みのガチャ模様）。
    pub selectable_pattern_ids: Vec<String>,
}

impl PatternStateDto {
    pub fn new(saved_pattern_id: &str, owned_gacha_pattern_ids: &[String]) -> Self {
        Self {
            active_pattern_id: effective_pattern_id(saved_pattern_id, owned_gacha_pattern_ids),
            selectable_pattern_ids: selectable_pattern_ids(owned_gacha_pattern_ids),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::reward::REWARD_MASTER;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn none_and_initial_patterns_are_always_selectable() {
        for id in ["none", "pattern_dots", "pattern_paw"] {
            assert!(is_selectable_pattern(id, &[]), "{id}");
        }
    }

    #[test]
    fn gacha_pattern_needs_ownership_and_other_ids_are_rejected() {
        let owned = ids(&[
            "gacha_pattern_sakura",
            "gacha_theme_001",
            "card_001",
            "mystery",
        ]);
        assert!(is_selectable_pattern("gacha_pattern_sakura", &owned));
        assert!(!is_selectable_pattern("gacha_pattern_snow", &owned));
        // 所持していてもテーマ・カードは模様として選べない。
        assert!(!is_selectable_pattern("gacha_theme_001", &owned));
        assert!(!is_selectable_pattern("card_001", &owned));
        assert!(!is_selectable_pattern("mystery", &owned));
        assert!(!is_selectable_pattern("", &owned));
        assert!(!is_selectable_pattern("default", &owned));
    }

    #[test]
    fn effective_pattern_falls_back_to_none() {
        let owned = ids(&["gacha_pattern_grid"]);
        assert_eq!(effective_pattern_id(" pattern_paw ", &owned), "pattern_paw");
        assert_eq!(
            effective_pattern_id("gacha_pattern_grid", &owned),
            "gacha_pattern_grid"
        );
        assert_eq!(effective_pattern_id("gacha_pattern_snow", &owned), "none");
        assert_eq!(effective_pattern_id("../etc", &owned), "none");
        assert_eq!(effective_pattern_id("", &[]), "none");
    }

    #[test]
    fn selectable_ids_follow_master_order_and_skip_unknown() {
        let owned = ids(&["gacha_pattern_snow", "mystery", "gacha_pattern_sparkle"]);
        assert_eq!(
            selectable_pattern_ids(&owned),
            ids(&[
                "none",
                "pattern_dots",
                "pattern_paw",
                "gacha_pattern_sparkle",
                "gacha_pattern_snow"
            ])
        );
    }

    #[test]
    fn initial_patterns_are_not_in_gacha_or_reward_masters() {
        for id in std::iter::once(PATTERN_NONE_ID).chain(INITIAL_PATTERN_IDS.iter().copied()) {
            assert!(GACHA_MASTER.iter().all(|def| def.item_id != id), "{id}");
            assert!(REWARD_MASTER.iter().all(|def| def.reward_id != id), "{id}");
        }
    }

    #[test]
    fn dto_uses_camel_case() {
        let json = serde_json::to_value(PatternStateDto::new("pattern_dots", &[])).unwrap();
        assert_eq!(json["activePatternId"], "pattern_dots");
        assert_eq!(json["selectablePatternIds"][0], "none");
    }
}
