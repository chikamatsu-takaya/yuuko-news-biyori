//! テーマの地に敷く模様のサービス（判断台帳 D152〜D154）。
//!
//! 模様の保存の正は設定 `ui.patternId`。選べるか（なし・初期所持・所持済みのガチャ模様）は
//! `domain/pattern.rs` で判定し、所持はガチャ状態（GachaService）から読む。
//! 色テーマ（RewardService::set_active_theme）と同じく、判定を通ったものだけを書き込み、
//! 読み取り時も選べない・未知の ID は「なし」へ倒す（設定ファイルの手編集・取り込みにも備える）。
//!
//! ガチャ状態はロックを取って読み終えてから設定を保存し、両方のロックを入れ子にしない。
//! 所持は増えるだけなので、判定後に設定を保存しても判定が覆ることはない。

use crate::domain::pattern::{is_selectable_pattern, PatternStateDto};
use crate::error::AppError;
use crate::services::gacha_service::GachaService;
use crate::services::settings_service::SettingsService;

#[derive(Debug, Clone)]
pub struct PatternService {
    settings_service: SettingsService,
    gacha_service: GachaService,
}

impl PatternService {
    pub fn new(settings_service: SettingsService, gacha_service: GachaService) -> Self {
        Self {
            settings_service,
            gacha_service,
        }
    }

    /// 適用中の模様と選べる模様の一覧を返す（読み取り）。
    /// ガチャ状態を読めないときは所持なし扱い（初期の模様だけ）で返し、表示は止めない。
    pub fn get_pattern_state(&self) -> PatternStateDto {
        let owned = self
            .gacha_service
            .owned_pattern_ids()
            .unwrap_or_else(|error| {
                log::warn!(
                    "ガチャ状態を読めなかったため、ガチャの模様は未所持として扱います: {error}"
                );
                Vec::new()
            });
        PatternStateDto::new(&self.settings_service.saved_pattern_id(), &owned)
    }

    /// 模様を切り替えて設定 `ui.patternId` に保存し、更新後の状態を返す（カスタマイズ画面）。
    /// 選べない模様（未所持・未知の ID）は検証エラーにして何も保存しない。
    pub fn set_active_pattern(&self, pattern_id: &str) -> Result<PatternStateDto, AppError> {
        // 所持を読めないときは、未所持扱いで拒否せずエラーとして返す（誤った案内を避ける）。
        let owned = self.gacha_service.owned_pattern_ids()?;
        if !is_selectable_pattern(pattern_id, &owned) {
            // ID そのものは外部由来の文字列のため、エラー文・ログに含めない。
            return Err(AppError::Validation("pattern is not owned".to_string()));
        }
        self.settings_service.set_pattern_id(pattern_id)?;
        Ok(PatternStateDto::new(pattern_id, &owned))
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
        service: PatternService,
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
            "yuuko-pattern-service-tests-{}-{n}",
            std::process::id()
        ));
        let paths = AppPaths::new(root.clone());
        let gacha_repository = GachaRepository::new(&paths);
        let settings = SettingsService::new(SettingsRepository::new(&paths));
        let service = PatternService::new(
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
    fn default_is_none_with_initial_patterns_selectable() {
        let ctx = ctx();
        let state = ctx.service.get_pattern_state();
        assert_eq!(state.active_pattern_id, "none");
        assert_eq!(
            state.selectable_pattern_ids,
            vec!["none", "pattern_dots", "pattern_paw"]
        );
    }

    #[test]
    fn initial_pattern_can_be_selected_and_persists() {
        let ctx = ctx();
        let state = ctx.service.set_active_pattern("pattern_paw").unwrap();
        assert_eq!(state.active_pattern_id, "pattern_paw");
        assert_eq!(ctx.settings.saved_pattern_id(), "pattern_paw");
        assert_eq!(
            ctx.service.get_pattern_state().active_pattern_id,
            "pattern_paw"
        );
    }

    #[test]
    fn unowned_or_unknown_pattern_is_rejected_without_saving() {
        let ctx = ctx();
        ctx.service.set_active_pattern("pattern_dots").unwrap();
        for id in [
            "gacha_pattern_snow",
            "gacha_theme_001",
            "mystery",
            "",
            " none",
        ] {
            let error = ctx.service.set_active_pattern(id).unwrap_err();
            assert!(matches!(error, AppError::Validation(_)), "{id}");
        }
        assert_eq!(ctx.settings.saved_pattern_id(), "pattern_dots");
    }

    #[test]
    fn owned_gacha_pattern_can_be_selected() {
        let ctx = ctx();
        own(&ctx, &["gacha_pattern_snow", "card_001"]);
        let state = ctx
            .service
            .set_active_pattern("gacha_pattern_snow")
            .unwrap();
        assert_eq!(state.active_pattern_id, "gacha_pattern_snow");
        assert!(state
            .selectable_pattern_ids
            .contains(&"gacha_pattern_snow".to_string()));
    }

    #[test]
    fn saved_but_unowned_or_unknown_pattern_reads_as_none() {
        // 設定ファイルの手編集・取り込みで未所持・未知の ID が入っても、適用は「なし」に倒す。
        let ctx = ctx();
        ctx.settings.set_pattern_id("gacha_pattern_heart").unwrap();
        assert_eq!(ctx.service.get_pattern_state().active_pattern_id, "none");
        ctx.settings.set_pattern_id("../../evil").unwrap();
        assert_eq!(ctx.service.get_pattern_state().active_pattern_id, "none");
        own(&ctx, &["gacha_pattern_heart"]);
        ctx.settings.set_pattern_id("gacha_pattern_heart").unwrap();
        assert_eq!(
            ctx.service.get_pattern_state().active_pattern_id,
            "gacha_pattern_heart"
        );
    }
}
