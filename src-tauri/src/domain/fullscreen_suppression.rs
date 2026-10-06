//! 全画面・プレゼン中の通知抑制と、抑制解除後の猶予（ゆうこ登場・通知挙動 詳細設計書 §5.2〜§5.4）。
//!
//! - 全画面中は通知を出さない。候補は選ばず・消費しないため、解除後の判定で同じ候補が再び選ばれる
//!   （§5.3「次回判定まで保留」）。
//! - 解除を観測したら 30〜180 秒のランダムな猶予を置き、その間も通知しない（§5.4）。
//! - 状態はメモリだけに持つ（非永続）。再起動で猶予が失われても、起動直後は判定まで時間が空くため
//!   実害は小さく、保存形式の変更を避けられる。

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Duration, Utc};

/// 抑制解除後の猶予の下限・上限（秒）。設計書 §5.4 の初期案。
pub const GRACE_MIN_SECONDS: i64 = 30;
pub const GRACE_MAX_SECONDS: i64 = 180;

/// 全画面抑制の判定結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenGate {
    Allowed,
    /// 全画面・プレゼン中。
    Suppressed,
    /// 解除後の猶予中（`until` まで通知しない）。
    GracePeriod {
        until: DateTime<Utc>,
    },
}

/// 直近に全画面を観測したか・猶予の終端を覚えておく（アプリ内通知とデスクトップ通知で共有する）。
#[derive(Debug, Clone, Default)]
pub struct FullscreenSuppressionTracker {
    suppressed: bool,
    grace_until: Option<DateTime<Utc>>,
}

impl FullscreenSuppressionTracker {
    /// 今回の観測結果を反映して判定する。
    ///
    /// `busy` は全画面・プレゼン中か（OS 判定に失敗した場合は false を渡す＝fail-open）。
    /// `grace` は「抑制中→非抑制」を観測したときに使う猶予。解除時刻は観測時刻で近似する
    /// （実際の解除はそれ以前のため、猶予が短くなる側には倒れない）。
    pub fn observe(&mut self, now: DateTime<Utc>, busy: bool, grace: Duration) -> FullscreenGate {
        if busy {
            self.suppressed = true;
            self.grace_until = None;
            return FullscreenGate::Suppressed;
        }

        if self.suppressed {
            self.suppressed = false;
            self.grace_until = Some(now + clamp_grace(grace));
        }

        match self.grace_until {
            // 時計が巻き戻っても上限を超えて止め続けないよう、残りは上限で頭打ちにする。
            Some(until) if now < until => {
                let until = until.min(now + Duration::seconds(GRACE_MAX_SECONDS));
                self.grace_until = Some(until);
                FullscreenGate::GracePeriod { until }
            }
            _ => {
                self.grace_until = None;
                FullscreenGate::Allowed
            }
        }
    }

    /// 設定で抑制が OFF になったときなど、保留中の抑制・猶予を捨てる。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 猶予中なら残り時間を返す（デスクトップ通知スレッドが次の判定時刻を決めるのに使う）。
    pub fn grace_remaining(&self, now: DateTime<Utc>) -> Option<Duration> {
        self.grace_until
            .filter(|until| now < *until)
            .map(|until| until - now)
    }
}

/// 猶予を 30〜180 秒に収める。
fn clamp_grace(grace: Duration) -> Duration {
    grace.clamp(
        Duration::seconds(GRACE_MIN_SECONDS),
        Duration::seconds(GRACE_MAX_SECONDS),
    )
}

/// シード値から 30〜180 秒の猶予を決める（テストではシードを固定して検証する）。
pub fn grace_from_seed(seed: u64) -> Duration {
    let span = (GRACE_MAX_SECONDS - GRACE_MIN_SECONDS + 1) as u64;
    let offset = (mix64(seed) % span) as i64;
    Duration::seconds(GRACE_MIN_SECONDS + offset)
}

/// 現在時刻のナノ秒からシードを作る。
///
/// 猶予は「解除直後に毎回同じ間隔で出る違和感」を避けるためのばらつきで、予測困難さは不要。
/// rand は依存ツリーに推移的に存在するが直接依存ではなく、追加すると依存追加になるため、
/// 時刻由来の値をビット拡散して使う。
pub fn time_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0)
}

/// splitmix64 の最終段。近い時刻のシードでも結果がばらけるようにする。
fn mix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 6, 10, 0, 0).unwrap() + Duration::seconds(seconds)
    }

    const GRACE: i64 = 90;

    fn observe(tracker: &mut FullscreenSuppressionTracker, s: i64, busy: bool) -> FullscreenGate {
        tracker.observe(at(s), busy, Duration::seconds(GRACE))
    }

    #[test]
    fn allowed_when_never_suppressed() {
        let mut tracker = FullscreenSuppressionTracker::default();
        assert_eq!(observe(&mut tracker, 0, false), FullscreenGate::Allowed);
        assert!(tracker.grace_remaining(at(0)).is_none());
    }

    #[test]
    fn suppressed_while_busy() {
        let mut tracker = FullscreenSuppressionTracker::default();
        assert_eq!(observe(&mut tracker, 0, true), FullscreenGate::Suppressed);
        assert_eq!(observe(&mut tracker, 60, true), FullscreenGate::Suppressed);
    }

    #[test]
    fn grace_period_starts_when_suppression_ends_and_then_allows() {
        let mut tracker = FullscreenSuppressionTracker::default();
        observe(&mut tracker, 0, true);

        // 解除を観測した時点から猶予が始まる。
        assert_eq!(
            observe(&mut tracker, 100, false),
            FullscreenGate::GracePeriod {
                until: at(100 + GRACE)
            }
        );
        // 猶予中は時間が経っても延長されない。
        assert_eq!(
            observe(&mut tracker, 150, false),
            FullscreenGate::GracePeriod {
                until: at(100 + GRACE)
            }
        );
        assert_eq!(
            tracker.grace_remaining(at(150)),
            Some(Duration::seconds(100 + GRACE - 150))
        );
        // 猶予が明けたら通知可。
        assert_eq!(
            observe(&mut tracker, 100 + GRACE, false),
            FullscreenGate::Allowed
        );
        assert!(tracker.grace_remaining(at(100 + GRACE)).is_none());
    }

    #[test]
    fn busy_again_during_grace_restarts_suppression() {
        let mut tracker = FullscreenSuppressionTracker::default();
        observe(&mut tracker, 0, true);
        observe(&mut tracker, 10, false);
        assert_eq!(observe(&mut tracker, 20, true), FullscreenGate::Suppressed);
        // 再解除時は新しい猶予が始まる。
        assert_eq!(
            observe(&mut tracker, 30, false),
            FullscreenGate::GracePeriod {
                until: at(30 + GRACE)
            }
        );
    }

    #[test]
    fn grace_is_clamped_to_design_range() {
        let mut tracker = FullscreenSuppressionTracker::default();
        tracker.observe(at(0), true, Duration::seconds(5));
        assert_eq!(
            tracker.observe(at(1), false, Duration::seconds(5)),
            FullscreenGate::GracePeriod {
                until: at(1 + GRACE_MIN_SECONDS)
            }
        );

        let mut tracker = FullscreenSuppressionTracker::default();
        tracker.observe(at(0), true, Duration::seconds(3600));
        assert_eq!(
            tracker.observe(at(1), false, Duration::seconds(3600)),
            FullscreenGate::GracePeriod {
                until: at(1 + GRACE_MAX_SECONDS)
            }
        );
    }

    #[test]
    fn clock_rollback_does_not_extend_grace_beyond_max() {
        let mut tracker = FullscreenSuppressionTracker::default();
        observe(&mut tracker, 1000, true);
        observe(&mut tracker, 1000, false);
        // 時計が 10 分巻き戻っても、残りは上限 180 秒まで。
        assert_eq!(
            observe(&mut tracker, 400, false),
            FullscreenGate::GracePeriod {
                until: at(400 + GRACE_MAX_SECONDS)
            }
        );
    }

    #[test]
    fn reset_discards_pending_suppression() {
        let mut tracker = FullscreenSuppressionTracker::default();
        observe(&mut tracker, 0, true);
        tracker.reset();
        assert_eq!(observe(&mut tracker, 1, false), FullscreenGate::Allowed);
    }

    #[test]
    fn grace_from_seed_stays_within_30_to_180_seconds_and_varies() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..2000u64 {
            let grace = grace_from_seed(seed).num_seconds();
            assert!((GRACE_MIN_SECONDS..=GRACE_MAX_SECONDS).contains(&grace));
            seen.insert(grace);
        }
        // 連続したシードでも値がばらける（ほぼ全域を使う）。
        assert!(seen.len() > 140, "grace values should vary: {}", seen.len());
    }
}
