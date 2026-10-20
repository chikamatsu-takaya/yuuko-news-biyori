//! ゆうこの着せかえ（吹き出し・飾り）のドメイン（判断台帳 D155）。
//!
//! - 吹き出し 10 種・飾り（アクセサリー）10 種。保存先は設定 `ui.balloonId` / `ui.decoId`。
//! - 最初から使えるのは吹き出し「いつもの」（`balloon_default`）と飾り「なし」（`deco_none`）だけ。
//!   残りはガチャの種類 `Balloon` / `Deco`（ガチャマスタ）で手に入れる。
//! - 既定は新規・既存ユーザーとも「いつもの」「なし」（今の見た目を変えない）。未知・未所持の ID は
//!   適用時に既定へ倒す。
//!
//! 吹き出しの形（CSS）と飾りの絵（SVG）はフロント（lib/yuuko-dressup.mjs・app/globals.css・
//! public/assets/accessories）が持ち、ここは「どの ID を選んでよいか」だけを決める（模様の `domain/pattern.rs` と同じ分担）。

use serde::Serialize;

use crate::domain::gacha::{find_gacha_item, GachaItemKind, GACHA_MASTER};

/// 吹き出し「いつもの」（既定）。lib/yuuko-dressup.mjs の BALLOON_DEFAULT_ID と一致させる。
pub const BALLOON_DEFAULT_ID: &str = "balloon_default";

/// 飾り「なし」（既定）。lib/yuuko-dressup.mjs の DECO_NONE_ID と一致させる。
pub const DECO_NONE_ID: &str = "deco_none";

/// 着せかえの部位。部位ごとに既定 ID とガチャの種類が決まる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DressupSlot {
    Balloon,
    Deco,
}

impl DressupSlot {
    /// 最初から使える既定の ID（ガチャには入れない）。
    pub const fn default_id(self) -> &'static str {
        match self {
            Self::Balloon => BALLOON_DEFAULT_ID,
            Self::Deco => DECO_NONE_ID,
        }
    }

    /// この部位の品物が属するガチャの種類。
    pub const fn gacha_kind(self) -> GachaItemKind {
        match self {
            Self::Balloon => GachaItemKind::Balloon,
            Self::Deco => GachaItemKind::Deco,
        }
    }
}

/// 利用者がこの部位に選んでよい ID か。既定・所持済みのガチャ品（この部位の種類）だけを許す
/// （React から任意の ID を書き込ませないため、判定は Rust に置く）。
/// `owned_gacha_ids` はガチャ状態の所持 ID。マスタにあるこの部位の種類の ID だけを有効とみなす。
pub fn is_selectable(slot: DressupSlot, item_id: &str, owned_gacha_ids: &[String]) -> bool {
    if item_id == slot.default_id() {
        return true;
    }
    find_gacha_item(item_id).is_some_and(|def| def.kind == slot.gacha_kind())
        && owned_gacha_ids.iter().any(|id| id == item_id)
}

/// この部位で選べる ID の一覧（既定 → ガチャマスタ順の所持済み）。
pub fn selectable_ids(slot: DressupSlot, owned_gacha_ids: &[String]) -> Vec<String> {
    let mut ids = vec![slot.default_id().to_string()];
    ids.extend(
        GACHA_MASTER
            .iter()
            .filter(|def| def.kind == slot.gacha_kind())
            .filter(|def| owned_gacha_ids.iter().any(|id| id == def.item_id))
            .map(|def| def.item_id.to_string()),
    );
    ids
}

/// 保存済みの ID を、適用してよい ID へ解決する。前後の空白は除き、選べない ID は既定へ倒す。
pub fn effective_id(slot: DressupSlot, saved_id: &str, owned_gacha_ids: &[String]) -> String {
    let trimmed = saved_id.trim();
    if is_selectable(slot, trimmed, owned_gacha_ids) {
        trimmed.to_string()
    } else {
        slot.default_id().to_string()
    }
}

/// `get_dressup_state` / `set_active_balloon` / `set_active_deco` の戻り値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DressupStateDto {
    /// 適用中の吹き出し ID（選べない・未知の ID は "balloon_default"）。
    pub active_balloon_id: String,
    /// いま選べる吹き出しの ID（"balloon_default"・所持済みのガチャ吹き出し）。
    pub selectable_balloon_ids: Vec<String>,
    /// 適用中の飾り ID（選べない・未知の ID は "deco_none"）。
    pub active_deco_id: String,
    /// いま選べる飾りの ID（"deco_none"・所持済みのガチャ飾り）。
    pub selectable_deco_ids: Vec<String>,
}

impl DressupStateDto {
    pub fn new(saved_balloon_id: &str, saved_deco_id: &str, owned_gacha_ids: &[String]) -> Self {
        Self {
            active_balloon_id: effective_id(
                DressupSlot::Balloon,
                saved_balloon_id,
                owned_gacha_ids,
            ),
            selectable_balloon_ids: selectable_ids(DressupSlot::Balloon, owned_gacha_ids),
            active_deco_id: effective_id(DressupSlot::Deco, saved_deco_id, owned_gacha_ids),
            selectable_deco_ids: selectable_ids(DressupSlot::Deco, owned_gacha_ids),
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
    fn defaults_are_always_selectable_only_in_their_own_slot() {
        assert!(is_selectable(DressupSlot::Balloon, "balloon_default", &[]));
        assert!(is_selectable(DressupSlot::Deco, "deco_none", &[]));
        // 部位をまたいだ既定 ID・模様の「なし」は選べない。
        assert!(!is_selectable(DressupSlot::Balloon, "deco_none", &[]));
        assert!(!is_selectable(DressupSlot::Deco, "balloon_default", &[]));
        assert!(!is_selectable(DressupSlot::Deco, "none", &[]));
    }

    #[test]
    fn gacha_items_need_ownership_and_matching_kind() {
        let owned = ids(&[
            "gacha_balloon_cloud",
            "gacha_deco_crown",
            "gacha_pattern_snow",
            "gacha_theme_001",
            "mystery",
        ]);
        assert!(is_selectable(
            DressupSlot::Balloon,
            "gacha_balloon_cloud",
            &owned
        ));
        assert!(!is_selectable(
            DressupSlot::Balloon,
            "gacha_balloon_seal",
            &owned
        ));
        assert!(is_selectable(DressupSlot::Deco, "gacha_deco_crown", &owned));
        assert!(!is_selectable(
            DressupSlot::Deco,
            "gacha_deco_ribbon",
            &owned
        ));
        // 所持していても別の部位・別の種類のものは選べない。
        assert!(!is_selectable(
            DressupSlot::Deco,
            "gacha_balloon_cloud",
            &owned
        ));
        assert!(!is_selectable(
            DressupSlot::Balloon,
            "gacha_deco_crown",
            &owned
        ));
        assert!(!is_selectable(
            DressupSlot::Balloon,
            "gacha_pattern_snow",
            &owned
        ));
        assert!(!is_selectable(DressupSlot::Deco, "gacha_theme_001", &owned));
        assert!(!is_selectable(DressupSlot::Deco, "mystery", &owned));
        assert!(!is_selectable(DressupSlot::Balloon, "", &owned));
    }

    #[test]
    fn effective_id_falls_back_to_default() {
        let owned = ids(&["gacha_deco_ribbon"]);
        assert_eq!(
            effective_id(DressupSlot::Deco, " gacha_deco_ribbon ", &owned),
            "gacha_deco_ribbon"
        );
        assert_eq!(
            effective_id(DressupSlot::Deco, "gacha_deco_crown", &owned),
            "deco_none"
        );
        assert_eq!(
            effective_id(DressupSlot::Balloon, "../etc", &owned),
            "balloon_default"
        );
        assert_eq!(
            effective_id(DressupSlot::Balloon, "", &[]),
            "balloon_default"
        );
    }

    #[test]
    fn selectable_ids_follow_master_order_and_skip_unknown() {
        let owned = ids(&[
            "gacha_deco_santa_hat",
            "mystery",
            "gacha_deco_ribbon",
            "gacha_balloon_seal",
            "gacha_balloon_rim",
        ]);
        assert_eq!(
            selectable_ids(DressupSlot::Deco, &owned),
            ids(&["deco_none", "gacha_deco_ribbon", "gacha_deco_santa_hat"])
        );
        assert_eq!(
            selectable_ids(DressupSlot::Balloon, &owned),
            ids(&["balloon_default", "gacha_balloon_rim", "gacha_balloon_seal"])
        );
    }

    #[test]
    fn defaults_are_not_in_gacha_or_reward_masters() {
        for id in [BALLOON_DEFAULT_ID, DECO_NONE_ID] {
            assert!(GACHA_MASTER.iter().all(|def| def.item_id != id), "{id}");
            assert!(REWARD_MASTER.iter().all(|def| def.reward_id != id), "{id}");
        }
    }

    #[test]
    fn dto_uses_camel_case() {
        let owned = ids(&["gacha_balloon_note"]);
        let json = serde_json::to_value(DressupStateDto::new(
            "gacha_balloon_note",
            "gacha_deco_crown",
            &owned,
        ))
        .unwrap();
        assert_eq!(json["activeBalloonId"], "gacha_balloon_note");
        assert_eq!(json["selectableBalloonIds"][1], "gacha_balloon_note");
        assert_eq!(json["activeDecoId"], "deco_none");
        assert_eq!(json["selectableDecoIds"][0], "deco_none");
    }
}
