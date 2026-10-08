//! ガチャサービス。ガチャ状態（gacha_state.json）の読み取り・1回引く・「NEW」を外す処理を担う
//! （データ設計書 §12・詳細設計書 §5.3.7 / §10.6）。
//!
//! 状態の読み込み→変更→保存は `update` に集め、ロックの中で1回の保存にまとめる
//! （かけらの消費と獲得の記録を片方だけ反映させない。§12.5）。かけらの付与（UyaDrMln）を足すときも
//! `update` の中で `GachaState::add_fragments` を呼べばよい。
//!
//! 抽選の乱数は差し替え可能（`with_picker`）にして、テストでは結果を固定する。

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::gacha::{
    find_gacha_item, DrawOutcome, GachaDrawResultDto, GachaState, GachaStateDto, GACHA_MASTER,
};
use crate::error::AppError;
use crate::repositories::gacha_repository::GachaRepository;

/// 抽選に使う関数。`n`（1 以上）を受け取り 0..n の添字を返す。
pub type GachaPicker = Arc<dyn Fn(usize) -> usize + Send + Sync>;

#[derive(Clone)]
pub struct GachaService {
    gacha_repository: GachaRepository,
    store_lock: Arc<Mutex<()>>,
    picker: GachaPicker,
}

impl GachaService {
    pub fn new(gacha_repository: GachaRepository) -> Self {
        Self::with_picker(gacha_repository, Arc::new(random_index))
    }

    /// 抽選関数を指定して作る（テストで結果を固定するため）。
    pub fn with_picker(gacha_repository: GachaRepository, picker: GachaPicker) -> Self {
        Self {
            gacha_repository,
            store_lock: Arc::new(Mutex::new(())),
            picker,
        }
    }

    /// ガチャ画面用の状態を返す（読み取り）。未保存なら初期値（初期かけら・所持なし）を返す。
    pub fn get_gacha_state(&self) -> Result<GachaStateDto, AppError> {
        self.update(|state| (state.to_dto(), false))
    }

    /// 1回引く（§12.5）。コンプリート・かけら不足のときは何も消費せず、その理由を結果で返す。
    /// 引けたときの保存に失敗したらエラーを返す（獲得したと誤認させない）。
    pub fn draw_once(&self) -> Result<GachaDrawResultDto, AppError> {
        let picker = Arc::clone(&self.picker);
        self.update(|state| {
            let outcome = state.draw(|n| picker(n));
            let changed = matches!(outcome, DrawOutcome::Drawn(_));
            (GachaDrawResultDto::from_outcome(outcome, state), changed)
        })
    }

    /// 指定した排出対象の「NEW」を外す（コレクションで確認したとき）。
    /// ID はマスタにあるものだけ受け付ける（任意の文字列を保存ファイルへ広げない）。
    pub fn mark_items_seen(&self, item_ids: &[String]) -> Result<GachaStateDto, AppError> {
        validate_item_ids(item_ids)?;
        self.update(|state| {
            let changed = state.mark_seen(item_ids) > 0;
            (state.to_dto(), changed)
        })
    }

    /// ロックの中で状態を読み、`apply` で変更し、必要なら1回だけ保存する。
    ///
    /// - `apply` は (戻り値, 保存が必要な変更をしたか) を返す。変更したのに保存できなければエラー。
    /// - 読み込み時の整合（`normalize`）だけの変更は、保存に失敗しても表示を止めない（次回に再試行）。
    fn update<R>(&self, apply: impl FnOnce(&mut GachaState) -> (R, bool)) -> Result<R, AppError> {
        let _guard = self.lock_store()?;
        let mut state = self.gacha_repository.load()?.unwrap_or_default();
        let normalized = state.normalize();
        let (result, changed) = apply(&mut state);
        if changed {
            self.gacha_repository.save(&state)?;
        } else if normalized {
            if let Err(error) = self.gacha_repository.save(&state) {
                log::warn!(
                    "ガチャ状態の整合結果を保存できませんでした（次回に再試行します）: {error}"
                );
            }
        }
        Ok(result)
    }

    fn lock_store(&self) -> Result<MutexGuard<'_, ()>, AppError> {
        self.store_lock
            .lock()
            .map_err(|_| AppError::Io(std::io::Error::other("gacha store lock was poisoned")))
    }
}

/// `mark_gacha_items_seen` に渡された ID を検証する。空・件数過多・マスタに無い ID は受け付けない。
fn validate_item_ids(item_ids: &[String]) -> Result<(), AppError> {
    if item_ids.is_empty() {
        return Err(AppError::Validation(
            "itemIds must contain at least one item".to_string(),
        ));
    }
    if item_ids.len() > GACHA_MASTER.len() {
        return Err(AppError::Validation(
            "itemIds has too many items".to_string(),
        ));
    }
    if item_ids.iter().any(|id| find_gacha_item(id).is_none()) {
        // 値そのものはエラー文・ログに出さない（外部由来の文字列のため）。
        return Err(AppError::Validation(
            "itemIds must contain only known gacha items".to_string(),
        ));
    }
    Ok(())
}

/// 標準ライブラリだけで 0..n の添字を作る（依存を増やさないため）。
///
/// `RandomState::new()` は呼ぶたびに異なる鍵のハッシャーを作るので、そこへ現在時刻を混ぜた値を使う。
/// 暗号用途ではなく、等確率に近い抽選ができれば足りる（n は高々数十のため剰余の偏りは無視できる）。
fn random_index(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut hasher = RandomState::new().build_hasher();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    hasher.write_u128(nanos);
    (hasher.finish() % n as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gacha::{GachaDrawStatus, GACHA_COST, INITIAL_FRAGMENTS};
    use crate::paths::AppPaths;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct Ctx {
        service: GachaService,
        repository: GachaRepository,
        paths: AppPaths,
    }

    impl Drop for Ctx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.paths.app_data_dir);
        }
    }

    /// 常に未所持の先頭を選ぶサービス。
    fn ctx() -> Ctx {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "yuuko-gacha-service-tests-{}-{n}",
            std::process::id()
        ));
        let paths = AppPaths::new(root);
        let repository = GachaRepository::new(&paths);
        let service = GachaService::with_picker(repository.clone(), Arc::new(|_| 0));
        Ctx {
            service,
            repository,
            paths,
        }
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn first_get_returns_initial_state_without_error() {
        let ctx = ctx();
        let dto = ctx.service.get_gacha_state().unwrap();
        assert_eq!(dto.star_fragments, INITIAL_FRAGMENTS);
        assert_eq!(dto.cost, GACHA_COST);
        assert!(dto.can_draw && !dto.is_complete);
        assert_eq!((dto.owned_count, dto.total_count), (0, 43));
    }

    #[test]
    fn draw_persists_consumption_and_item_together() {
        let ctx = ctx();
        let result = ctx.service.draw_once().unwrap();
        assert_eq!(result.status, GachaDrawStatus::Drawn);
        let item = result.item.unwrap();
        assert_eq!(item.item_id, "card_001");
        assert!(item.text.is_some());
        assert_eq!(result.star_fragments, INITIAL_FRAGMENTS - GACHA_COST);

        let saved = ctx.repository.load().unwrap().unwrap();
        assert_eq!(saved.star_fragments, INITIAL_FRAGMENTS - GACHA_COST);
        assert_eq!(saved.owned_item_ids, ids(&["card_001"]));
        assert_eq!(saved.new_item_ids, ids(&["card_001"]));

        let dto = ctx.service.get_gacha_state().unwrap();
        assert_eq!(dto.owned_count, 1);
        assert!(dto.items[0].owned && dto.items[0].is_new);
        assert!(!dto.can_draw);
    }

    #[test]
    fn insufficient_draw_returns_reason_and_saves_nothing() {
        let ctx = ctx();
        ctx.service.draw_once().unwrap();
        let before = std::fs::read(&ctx.paths.gacha_state_path).unwrap();

        let result = ctx.service.draw_once().unwrap();
        assert_eq!(result.status, GachaDrawStatus::Insufficient);
        assert!(result.item.is_none());
        assert_eq!(result.star_fragments, 0);
        assert_eq!(std::fs::read(&ctx.paths.gacha_state_path).unwrap(), before);
    }

    #[test]
    fn complete_draw_returns_reason_and_keeps_fragments() {
        let ctx = ctx();
        let mut state = GachaState {
            owned_item_ids: GACHA_MASTER
                .iter()
                .map(|def| def.item_id.to_string())
                .collect(),
            ..GachaState::default()
        };
        state.add_fragments(100);
        ctx.repository.save(&state).unwrap();

        let result = ctx.service.draw_once().unwrap();
        assert_eq!(result.status, GachaDrawStatus::Complete);
        assert!(result.is_complete && result.item.is_none());
        assert_eq!(result.star_fragments, INITIAL_FRAGMENTS + 100);
        assert_eq!(ctx.repository.load().unwrap().unwrap(), state);
    }

    #[test]
    fn drawing_until_complete_never_repeats_items() {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "yuuko-gacha-service-tests-{}-{n}",
            std::process::id()
        ));
        let paths = AppPaths::new(root.clone());
        let repository = GachaRepository::new(&paths);
        // 実際の乱数で最後まで引いても重複せず、43 回でコンプリートする。
        let service = GachaService::new(repository.clone());
        let mut state = GachaState::default();
        state.add_fragments(GACHA_COST * 43);
        repository.save(&state).unwrap();

        let mut drawn = Vec::new();
        for _ in 0..43 {
            let result = service.draw_once().unwrap();
            assert_eq!(result.status, GachaDrawStatus::Drawn);
            drawn.push(result.item.unwrap().item_id);
        }
        drawn.sort();
        drawn.dedup();
        assert_eq!(drawn.len(), 43);
        assert_eq!(
            service.draw_once().unwrap().status,
            GachaDrawStatus::Complete
        );
        assert!(service.get_gacha_state().unwrap().is_complete);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_state_is_reset_and_backed_up_before_drawing() {
        let ctx = ctx();
        std::fs::create_dir_all(ctx.paths.gacha_state_path.parent().unwrap()).unwrap();
        std::fs::write(&ctx.paths.gacha_state_path, b"{ broken").unwrap();

        let result = ctx.service.draw_once().unwrap();
        assert_eq!(result.status, GachaDrawStatus::Drawn);
        assert_eq!(
            std::fs::read(ctx.paths.gacha_state_path.with_extension("corrupt.json")).unwrap(),
            b"{ broken"
        );
    }

    #[test]
    fn mark_seen_clears_new_and_rejects_unknown_ids() {
        let ctx = ctx();
        ctx.service.draw_once().unwrap();

        assert!(ctx.service.mark_items_seen(&[]).is_err());
        assert!(ctx
            .service
            .mark_items_seen(&ids(&["../../etc/passwd"]))
            .is_err());
        assert!(ctx
            .service
            .mark_items_seen(&ids(&["card_001", "theme_001"]))
            .is_err());
        let too_many: Vec<String> = (0..=GACHA_MASTER.len())
            .map(|_| "card_001".to_string())
            .collect();
        assert!(ctx.service.mark_items_seen(&too_many).is_err());
        // 検証で弾いたときは何も変えない。
        assert_eq!(
            ctx.repository.load().unwrap().unwrap().new_item_ids,
            ids(&["card_001"])
        );

        // 未所持の ID を含めてもよい（何もしない）。
        let dto = ctx
            .service
            .mark_items_seen(&ids(&["card_001", "card_002"]))
            .unwrap();
        assert!(dto.items[0].owned && !dto.items[0].is_new);
        let saved = ctx.repository.load().unwrap().unwrap();
        assert!(saved.new_item_ids.is_empty());
        assert_eq!(saved.owned_item_ids, ids(&["card_001"]));
    }

    #[test]
    fn random_index_stays_in_range() {
        for n in 1..=50 {
            for _ in 0..20 {
                assert!(random_index(n) < n);
            }
        }
        assert_eq!(random_index(0), 0);
    }
}
