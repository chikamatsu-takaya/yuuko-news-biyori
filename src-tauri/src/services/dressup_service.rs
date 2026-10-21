//! ゆうこの着せかえ（吹き出し・飾り）のサービス（判断台帳 D155）。
//!
//! 保存の正は設定 `ui.balloonId` / `ui.decoId`。選べるか（既定・所持済みのガチャ品）は
//! `domain/dressup.rs` で判定し、所持はガチャ状態（GachaService）から読む。
//! 模様（PatternService）と同じく、判定を通ったものだけを書き込み、読み取り時も選べない・未知の ID は
//! 既定へ倒す（設定ファイルの手編集・取り込みにも備える）。
//!
//! ガチャ状態はロックを取って読み終えてから設定を保存し、両方のロックを入れ子にしない。
//! 所持は増えるだけなので、判定後に設定を保存しても判定が覆ることはない。

use crate::domain::dressup::{is_selectable, ActiveDressupIds, DressupSlot, DressupStateDto};
use crate::error::AppError;
use crate::services::gacha_service::GachaService;
use crate::services::settings_service::SettingsService;

#[derive(Debug, Clone)]
pub struct DressupService {
    settings_service: SettingsService,
    gacha_service: GachaService,
}

impl DressupService {
    pub fn new(settings_service: SettingsService, gacha_service: GachaService) -> Self {
        Self {
            settings_service,
            gacha_service,
        }
    }

    /// 適用中の吹き出し・飾りと、選べるものの一覧を返す（読み取り）。
    /// ガチャ状態を読めないときは所持なし扱い（既定だけ）で返し、表示は止めない。
    pub fn get_dressup_state(&self) -> DressupStateDto {
        let owned = self
            .gacha_service
            .owned_dressup_ids()
            .unwrap_or_else(|error| {
                log::warn!(
                    "ガチャ状態を読めなかったため、ガチャの着せかえは未所持として扱います: {error}"
                );
                Vec::new()
            });
        let (balloon_id, deco_id) = self.settings_service.saved_dressup_ids();
        DressupStateDto::new(&balloon_id, &deco_id, &owned)
    }

    /// 適用中の吹き出し・飾りの ID だけを返す（常駐ゆうこウィンドウへ通知データで渡す用。判断台帳 D156）。
    /// 解決は get_dressup_state と同じ（未所持・未知は既定、ガチャ状態を読めなければ既定）。
    pub fn active_dressup_ids(&self) -> ActiveDressupIds {
        let state = self.get_dressup_state();
        ActiveDressupIds {
            balloon_id: state.active_balloon_id,
            deco_id: state.active_deco_id,
        }
    }

    /// 吹き出しを切り替えて設定 `ui.balloonId` に保存し、更新後の状態を返す（カスタマイズ画面）。
    /// 選べない吹き出し（未所持・未知の ID）は検証エラーにして何も保存しない。
    pub fn set_active_balloon(&self, balloon_id: &str) -> Result<DressupStateDto, AppError> {
        self.set_active(DressupSlot::Balloon, balloon_id)
    }

    /// 飾りを切り替えて設定 `ui.decoId` に保存し、更新後の状態を返す（カスタマイズ画面）。
    /// 選べない飾り（未所持・未知の ID）は検証エラーにして何も保存しない。
    pub fn set_active_deco(&self, deco_id: &str) -> Result<DressupStateDto, AppError> {
        self.set_active(DressupSlot::Deco, deco_id)
    }

    fn set_active(&self, slot: DressupSlot, item_id: &str) -> Result<DressupStateDto, AppError> {
        // 所持を読めないときは、未所持扱いで拒否せずエラーとして返す（誤った案内を避ける）。
        let owned = self.gacha_service.owned_dressup_ids()?;
        if !is_selectable(slot, item_id, &owned) {
            // ID そのものは外部由来の文字列のため、エラー文・ログに含めない。
            return Err(AppError::Validation(
                "dressup item is not owned".to_string(),
            ));
        }
        match slot {
            DressupSlot::Balloon => self.settings_service.set_balloon_id(item_id)?,
            DressupSlot::Deco => self.settings_service.set_deco_id(item_id)?,
        }
        let (balloon_id, deco_id) = self.settings_service.saved_dressup_ids();
        Ok(DressupStateDto::new(&balloon_id, &deco_id, &owned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gacha::GachaState;
    use crate::paths::AppPaths;
    use crate::repositories::gacha_repository::GachaRepository;
    use crate::repositories::settings_repository::SettingsRepository;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct Ctx {
        service: DressupService,
        settings: SettingsService,
        gacha_repository: GachaRepository,
        root: std::path::PathBuf,
    }

    impl Drop for Ctx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn ctx() -> Ctx {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "yuuko-dressup-service-tests-{}-{n}",
            std::process::id()
        ));
        let paths = AppPaths::new(root.clone());
        let gacha_repository = GachaRepository::new(&paths);
        let settings = SettingsService::new(SettingsRepository::new(&paths));
        let service = DressupService::new(
            settings.clone(),
            GachaService::new(gacha_repository.clone()),
        );
        Ctx {
            service,
            settings,
            gacha_repository,
            root,
        }
    }

    fn own(ctx: &Ctx, ids: &[&str]) {
        let state = GachaState {
            owned_item_ids: ids.iter().map(|id| id.to_string()).collect(),
            ..GachaState::default()
        };
        ctx.gacha_repository.save(&state).unwrap();
    }

    #[test]
    fn default_state_only_offers_the_defaults() {
        let ctx = ctx();
        let state = ctx.service.get_dressup_state();
        assert_eq!(state.active_balloon_id, "balloon_default");
        assert_eq!(state.selectable_balloon_ids, vec!["balloon_default"]);
        assert_eq!(state.active_deco_id, "deco_none");
        assert_eq!(state.selectable_deco_ids, vec!["deco_none"]);
    }

    #[test]
    fn owned_items_can_be_selected_and_persist_independently() {
        let ctx = ctx();
        own(
            &ctx,
            &["gacha_balloon_cloud", "gacha_deco_crown", "card_001"],
        );
        let state = ctx
            .service
            .set_active_balloon("gacha_balloon_cloud")
            .unwrap();
        assert_eq!(state.active_balloon_id, "gacha_balloon_cloud");
        assert_eq!(state.active_deco_id, "deco_none");
        let state = ctx.service.set_active_deco("gacha_deco_crown").unwrap();
        assert_eq!(state.active_balloon_id, "gacha_balloon_cloud");
        assert_eq!(state.active_deco_id, "gacha_deco_crown");
        assert_eq!(
            state.selectable_deco_ids,
            vec!["deco_none", "gacha_deco_crown"]
        );
        let reread = ctx.service.get_dressup_state();
        assert_eq!(reread, state);
        // 既定へ戻せる。
        let state = ctx.service.set_active_deco("deco_none").unwrap();
        assert_eq!(state.active_deco_id, "deco_none");
    }

    #[test]
    fn unowned_unknown_or_wrong_slot_ids_are_rejected_without_saving() {
        let ctx = ctx();
        own(&ctx, &["gacha_balloon_note", "gacha_deco_ribbon"]);
        ctx.service
            .set_active_balloon("gacha_balloon_note")
            .unwrap();
        ctx.service.set_active_deco("gacha_deco_ribbon").unwrap();
        for id in [
            "gacha_balloon_seal",
            "gacha_deco_ribbon",
            "deco_none",
            "gacha_pattern_snow",
            "mystery",
            "",
            " balloon_default",
        ] {
            let error = ctx.service.set_active_balloon(id).unwrap_err();
            assert!(matches!(error, AppError::Validation(_)), "{id}");
        }
        for id in [
            "gacha_deco_crown",
            "gacha_balloon_note",
            "balloon_default",
            "none",
        ] {
            let error = ctx.service.set_active_deco(id).unwrap_err();
            assert!(matches!(error, AppError::Validation(_)), "{id}");
        }
        assert_eq!(
            ctx.settings.saved_dressup_ids(),
            (
                "gacha_balloon_note".to_string(),
                "gacha_deco_ribbon".to_string()
            )
        );
    }

    #[test]
    fn saved_but_unowned_or_unknown_ids_read_as_defaults() {
        // 設定ファイルの手編集・取り込みで未所持・未知の ID が入っても、適用は既定に倒す。
        let ctx = ctx();
        ctx.settings.set_balloon_id("gacha_balloon_game").unwrap();
        ctx.settings.set_deco_id("../../evil").unwrap();
        let state = ctx.service.get_dressup_state();
        assert_eq!(state.active_balloon_id, "balloon_default");
        assert_eq!(state.active_deco_id, "deco_none");
        own(&ctx, &["gacha_balloon_game"]);
        assert_eq!(
            ctx.service.get_dressup_state().active_balloon_id,
            "gacha_balloon_game"
        );
    }

    #[test]
    fn active_dressup_ids_falls_back_to_defaults_like_get_dressup_state() {
        let ctx = ctx();
        assert_eq!(
            ctx.service.active_dressup_ids(),
            ActiveDressupIds::default()
        );
        // 未所持・未知の ID は既定に倒す。
        ctx.settings.set_balloon_id("gacha_balloon_seal").unwrap();
        ctx.settings.set_deco_id("mystery").unwrap();
        assert_eq!(
            ctx.service.active_dressup_ids(),
            ActiveDressupIds::default()
        );
        // 所持済みならそのまま渡す。
        own(&ctx, &["gacha_balloon_seal", "gacha_deco_crown"]);
        ctx.settings.set_deco_id("gacha_deco_crown").unwrap();
        assert_eq!(
            ctx.service.active_dressup_ids(),
            ActiveDressupIds {
                balloon_id: "gacha_balloon_seal".to_string(),
                deco_id: "gacha_deco_crown".to_string(),
            }
        );
    }
}
