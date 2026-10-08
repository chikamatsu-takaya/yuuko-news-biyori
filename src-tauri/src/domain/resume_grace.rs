//! PC スリープ復帰直後の通知猶予（ゆうこ登場・通知挙動 詳細設計書 §5.2 / §5.4）。
//!
//! - OS の電源イベントは使わない。デスクトップ通知スレッドは表示状態に関係なく一定間隔で回り続けるため、
//!   その「次はこの時間内に回るはず」という心拍と、実際に経過した壁時計の時間を比べ、想定より大幅に
//!   空いていたらスリープ復帰とみなす。単調時計はスリープ中に進まない環境があるため壁時計（UTC）を使う。
//! - アプリ内通知（React の 5 分ポーリング）はウィンドウ非表示中に止まるため、その間隔は判定に使わない
//!   （長い空白がスリープとは限らない）。判定の基準は常にデスクトップ通知スレッドの心拍だけにする。
//! - 復帰を検知したら全画面解除後と同じ 30〜180 秒の猶予を置き、その間は通知しない。候補は選ばず・
//!   消費しないため、猶予明けの判定で同じ候補が再び選ばれる（§5.3「次回判定まで保留」）。
//! - 状態はメモリだけに持つ（非永続）。

use chrono::{DateTime, Duration, Utc};

use crate::domain::fullscreen_suppression::{clamp_grace, GRACE_MAX_SECONDS};

/// 想定間隔に上乗せする余裕（秒）。判定の遅れや処理時間の揺れをスリープと誤認しないよう、
/// 想定間隔（最大 5 分）より十分大きい 10 分を足した時間を超えたときだけ復帰とみなす。
pub const RESUME_GAP_MARGIN_SECONDS: i64 = 10 * 60;

/// 前回の心拍から `now` までの空白が、想定間隔＋余裕を超えているか（スリープ復帰とみなすか）。
/// 時計が巻き戻った場合（空白が負）は復帰とみなさない。
pub fn is_resume_gap(
    last_heartbeat: DateTime<Utc>,
    now: DateTime<Utc>,
    expected: Duration,
) -> bool {
    now - last_heartbeat > expected + Duration::seconds(RESUME_GAP_MARGIN_SECONDS)
}

/// 復帰猶予の判定結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeGate {
    Allowed,
    /// 復帰後の猶予中（`until` まで通知しない）。
    GracePeriod {
        until: DateTime<Utc>,
    },
}

/// 心拍と猶予の終端を覚えておく（アプリ内通知とデスクトップ通知で共有する）。
#[derive(Debug, Clone, Default)]
pub struct ResumeGraceTracker {
    /// 直近の心拍時刻と、次の心拍までの想定間隔。
    last_heartbeat: Option<(DateTime<Utc>, Duration)>,
    grace_until: Option<DateTime<Utc>>,
}

impl ResumeGraceTracker {
    /// デスクトップ通知スレッドの心拍を記録する。`expected_next` は次の判定までの待ち時間。
    pub fn heartbeat(&mut self, now: DateTime<Utc>, expected_next: Duration) {
        self.last_heartbeat = Some((now, expected_next));
    }

    /// 復帰を検知していれば猶予を始め、猶予中かどうかを返す。
    ///
    /// 心拍がまだ無い（スレッド未開始・テスト）場合は検知しない。検知時は検知時刻を新しい基準にし、
    /// 同じ空白をアプリ内通知とデスクトップ通知の両方で二重に検知して猶予を延ばさないようにする。
    pub fn observe(&mut self, now: DateTime<Utc>, grace: Duration) -> ResumeGate {
        if let Some((last, expected)) = self.last_heartbeat {
            if is_resume_gap(last, now, expected) {
                self.grace_until = Some(now + clamp_grace(grace));
                self.last_heartbeat = Some((now, expected));
            }
        }

        match self.grace_until {
            // 時計が巻き戻っても上限を超えて止め続けないよう、残りは上限で頭打ちにする。
            Some(until) if now < until => {
                let until = until.min(now + Duration::seconds(GRACE_MAX_SECONDS));
                self.grace_until = Some(until);
                ResumeGate::GracePeriod { until }
            }
            _ => {
                self.grace_until = None;
                ResumeGate::Allowed
            }
        }
    }

    /// 猶予中なら残り時間を返す（デスクトップ通知スレッドが次の判定時刻を決めるのに使う）。
    pub fn grace_remaining(&self, now: DateTime<Utc>) -> Option<Duration> {
        self.grace_until
            .filter(|until| now < *until)
            .map(|until| until - now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::fullscreen_suppression::GRACE_MIN_SECONDS;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 8, 10, 0, 0).unwrap() + Duration::seconds(seconds)
    }

    const INTERVAL: i64 = 5 * 60;
    const GRACE: i64 = 90;

    fn observe(tracker: &mut ResumeGraceTracker, s: i64) -> ResumeGate {
        tracker.observe(at(s), Duration::seconds(GRACE))
    }

    fn tracker_with_heartbeat_at(s: i64) -> ResumeGraceTracker {
        let mut tracker = ResumeGraceTracker::default();
        tracker.heartbeat(at(s), Duration::seconds(INTERVAL));
        tracker
    }

    #[test]
    fn resume_gap_requires_expected_interval_plus_margin() {
        let expected = Duration::seconds(INTERVAL);
        let threshold = INTERVAL + RESUME_GAP_MARGIN_SECONDS;
        assert!(!is_resume_gap(at(0), at(INTERVAL), expected));
        assert!(!is_resume_gap(at(0), at(threshold), expected));
        assert!(is_resume_gap(at(0), at(threshold + 1), expected));
        assert!(is_resume_gap(at(0), at(8 * 3600), expected));
        // 時計の巻き戻しは復帰とみなさない。
        assert!(!is_resume_gap(at(0), at(-3600), expected));
    }

    #[test]
    fn allowed_without_heartbeat_even_after_long_time() {
        let mut tracker = ResumeGraceTracker::default();
        assert_eq!(observe(&mut tracker, 0), ResumeGate::Allowed);
        assert_eq!(observe(&mut tracker, 8 * 3600), ResumeGate::Allowed);
    }

    #[test]
    fn allowed_while_heartbeat_arrives_on_time() {
        let mut tracker = tracker_with_heartbeat_at(0);
        assert_eq!(observe(&mut tracker, INTERVAL + 30), ResumeGate::Allowed);
        tracker.heartbeat(at(INTERVAL + 30), Duration::seconds(INTERVAL));
        assert_eq!(
            observe(&mut tracker, 2 * INTERVAL + 60),
            ResumeGate::Allowed
        );
        assert!(tracker.grace_remaining(at(2 * INTERVAL + 60)).is_none());
    }

    #[test]
    fn long_gap_starts_grace_and_then_allows() {
        let mut tracker = tracker_with_heartbeat_at(0);
        let resumed = 2 * 3600;

        assert_eq!(
            observe(&mut tracker, resumed),
            ResumeGate::GracePeriod {
                until: at(resumed + GRACE)
            }
        );
        // 猶予中に再度観測しても、同じ空白で猶予を延ばさない。
        assert_eq!(
            observe(&mut tracker, resumed + 30),
            ResumeGate::GracePeriod {
                until: at(resumed + GRACE)
            }
        );
        assert_eq!(
            tracker.grace_remaining(at(resumed + 30)),
            Some(Duration::seconds(GRACE - 30))
        );
        assert_eq!(observe(&mut tracker, resumed + GRACE), ResumeGate::Allowed);
        assert!(tracker.grace_remaining(at(resumed + GRACE)).is_none());
    }

    #[test]
    fn expected_interval_follows_latest_heartbeat() {
        // 全画面中の 60 秒間隔なら、想定 60 秒＋余裕を超えた時点で復帰とみなす。
        let mut tracker = ResumeGraceTracker::default();
        tracker.heartbeat(at(0), Duration::seconds(60));
        assert_eq!(
            observe(&mut tracker, 60 + RESUME_GAP_MARGIN_SECONDS),
            ResumeGate::Allowed
        );
        let resumed = 60 + RESUME_GAP_MARGIN_SECONDS + 1;
        assert!(matches!(
            observe(&mut tracker, resumed),
            ResumeGate::GracePeriod { .. }
        ));
    }

    #[test]
    fn grace_is_clamped_to_design_range() {
        let mut tracker = tracker_with_heartbeat_at(0);
        assert_eq!(
            tracker.observe(at(3600), Duration::seconds(1)),
            ResumeGate::GracePeriod {
                until: at(3600 + GRACE_MIN_SECONDS)
            }
        );

        let mut tracker = tracker_with_heartbeat_at(0);
        assert_eq!(
            tracker.observe(at(3600), Duration::seconds(3600)),
            ResumeGate::GracePeriod {
                until: at(3600 + GRACE_MAX_SECONDS)
            }
        );
    }

    #[test]
    fn clock_rollback_does_not_extend_grace_beyond_max() {
        let mut tracker = tracker_with_heartbeat_at(0);
        observe(&mut tracker, 3600);
        // 時計が 10 分巻き戻っても、残りは上限 180 秒まで。
        assert_eq!(
            observe(&mut tracker, 3000),
            ResumeGate::GracePeriod {
                until: at(3000 + GRACE_MAX_SECONDS)
            }
        );
    }
}
