//! 通知モード（settings.json の notification.mode）による自動通知のタイミング判定（判断台帳 D143）。
//!
//! 日次上限・クールタイム・閉じる/無視後の抑制・時間帯（PersistedYuukoState::can_notify）を通った後に、
//! 「今この判定で出してよいか」をモードごとに決める。判定の呼び出しは既存の定期判定（非表示中は5分間隔、
//! アプリ内通知は画面側の定期確認）のままで、判定の間隔は増やさない。
//!
//! - 通知しない（`none`）: 自動通知を出さない（サービス層で通知OFFと同じ `disabled` を返す）。
//!   トレイ「ゆうこを呼ぶ」（D140）は利用者の明示操作で、この判定を通らないため使える。
//! - ランダム（`random`）: 通知時間帯（workTimeRanges）を使わず、固定の 7:00〜23:00。クールタイム明けからさらにランダムに待つ。
//! - 時間帯内ランダム（`random_in_work_time`・既定）: 通知時間帯（workTimeRanges）の中だけ。待ち方はランダムと同じ。
//! - 定時（`fixed_time`）: 各通知時間帯の開始時刻に1回ずつ。開始から一定時間（既定60分）の間に
//!   全画面・会議中などで出せなかった場合だけ遅れて出し、それを過ぎたらその回は見送る。
//!
//! ランダムの待ち時間は時刻由来のシードから決める（全画面解除後の猶予と同じ方式。予測困難さは不要）。
//! テストではシード・時刻・タイムゾーンを注入して結果を固定する。

use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};

use crate::domain::fullscreen_suppression::mix64;
use crate::domain::yuuko::{
    format_timestamp, parse_hour_minute, parse_timestamp, PersistedYuukoState,
};

/// クールタイム明け（またはその日・その時間帯の最初の判定）から足すランダムな待ちの上限（分）。
/// 0〜90分の一様な待ちにより、通知が「時間帯の開始直後」「クールタイムぴったり」にそろわないようにする。
pub const RANDOM_DELAY_MAX_MINUTES: i64 = 90;

/// 自動通知の判定間隔（分）。非表示中のデスクトップ通知スレッドの通常間隔
/// （`yuuko_desktop_notifier::CHECK_INTERVAL` = 5分）と一致させる。間隔を変えたらここも変える。
///
/// 判定は最大この間隔おきにしか来ないため、時間帯の終わり間際に決めた時刻だと、その時刻を過ぎた
/// 次の判定がもう時間帯の外になり出しそびれる。ランダムな待ちの上限を「時間帯の残り − この間隔」に
/// 抑え、決めた時刻の後に少なくとも1回は時間帯内の判定が来るようにする。
pub const POLL_INTERVAL_MINUTES: i64 = 5;

/// 定時モードで、開始時刻に出せなかった通知を遅れて出してよい時間（分）。
/// これを過ぎたらその回は見送る（「定時」から大きくずれた時刻に出さないため）。
pub const FIXED_TIME_WINDOW_MINUTES: i64 = 60;

/// ランダム（通知時間帯を使わない）で通知してよい時間（7:00〜23:00・固定。判断台帳 D143）。
/// 深夜・早朝に出さないための一般的な範囲で、利用者の通知時間帯とは独立。既存の時間帯判定は終端を含む（23:00 台の0分まで）。
pub const RANDOM_MODE_RANGE: (&str, &str) = ("07:00", "23:00");

const MINUTES_PER_DAY: i64 = 24 * 60;

/// 設定値 notification.mode の解釈。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationMode {
    /// ランダム（7:00〜23:00）。
    Random,
    /// 指定時間帯内でランダム（既定）。
    RandomInWorkTime,
    /// 定時（各時間帯の開始時刻）。
    FixedTime,
    /// 通知しない（自動通知なし）。
    Off,
}

impl NotificationMode {
    /// 保存値から読む。未知の値（手編集など）は既定の「時間帯内ランダム」として扱う
    /// （従来の挙動に近く、時間帯の外へは出さない側）。
    pub fn from_storage(value: &str) -> Self {
        match value {
            "random" => Self::Random,
            "fixed_time" => Self::FixedTime,
            "none" => Self::Off,
            _ => Self::RandomInWorkTime,
        }
    }

    /// 保存値として正しい場合だけ返す（設定保存時の検証用。未知の値は None）。
    pub fn parse_storage(value: &str) -> Option<Self> {
        match value {
            "random_in_work_time" => Some(Self::RandomInWorkTime),
            "random" => Some(Self::Random),
            "fixed_time" => Some(Self::FixedTime),
            "none" => Some(Self::Off),
            _ => None,
        }
    }

    /// 保存値（settings.json の notification.mode / 画面 DTO の notificationMode）。
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::RandomInWorkTime => "random_in_work_time",
            Self::Random => "random",
            Self::FixedTime => "fixed_time",
            Self::Off => "none",
        }
    }

    /// このモードで使う時間帯。ランダムだけ保存済みの通知時間帯を使わず、固定の 7:00〜23:00 にする。
    pub fn time_ranges<'a>(
        self,
        work_time_ranges: &[(&'a str, &'a str)],
    ) -> Vec<(&'a str, &'a str)> {
        match self {
            Self::Random => vec![RANDOM_MODE_RANGE],
            _ => work_time_ranges.to_vec(),
        }
    }
}

/// モードによるタイミング判定の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeTimingGate {
    /// 今出してよい。
    Allowed,
    /// ランダムな待ちの途中（理由は cooling_down として返す）。
    Waiting,
    /// 定時の回の外（理由は outside_time_range として返す）。
    OutsideSlot,
}

/// シードから 0〜`max_minutes` 分の待ちを一様に決める（分単位）。
///
/// 上限ごとに剰余を取るのではなく、シードを [0, 1) の比率として上限へ掛ける。こうすると
/// 「待ち0分になるシード」はどの上限でも0分になり、テストで上限に依存せず固定できる。
pub fn random_delay_from_seed(seed: u64, max_minutes: i64) -> Duration {
    let span = max_minutes.max(0) as u128 + 1;
    let minutes = (u128::from(mix64(seed)) * span) >> 64;
    Duration::minutes(minutes as i64)
}

/// モードごとのタイミング判定（本番はローカルタイムゾーン）。
/// 戻り値の bool は状態（次に通知してよい時刻）を更新したか。true なら呼び出し側が保存する。
pub fn check_mode_timing(
    state: &mut PersistedYuukoState,
    mode: NotificationMode,
    now: DateTime<Utc>,
    time_ranges: &[(&str, &str)],
    seed: u64,
) -> (ModeTimingGate, bool) {
    check_mode_timing_in(&chrono::Local, state, mode, now, time_ranges, seed)
}

/// `check_mode_timing` の本体。タイムゾーンを注入できるようにしてテストを端末設定に依存させない。
fn check_mode_timing_in<Tz: TimeZone>(
    tz: &Tz,
    state: &mut PersistedYuukoState,
    mode: NotificationMode,
    now: DateTime<Utc>,
    time_ranges: &[(&str, &str)],
    seed: u64,
) -> (ModeTimingGate, bool) {
    match mode {
        NotificationMode::Random | NotificationMode::RandomInWorkTime => {
            random_timing_in(tz, state, now, time_ranges, seed)
        }
        NotificationMode::FixedTime => (fixed_time_timing_in(tz, state, now, time_ranges), false),
        // 通知しないはサービス層で先に止める。ここへ来ても出さない側へ倒す。
        NotificationMode::Off => (ModeTimingGate::OutsideSlot, false),
    }
}

/// ランダム系モード: 保存済みの「次に通知してよい時刻」が有効ならそれを待ち、無効なら今からランダムに決め直す。
///
/// 待ちは 0〜min(90分, 今いる時間帯の終わりまでの残り − 判定間隔5分) 分（0未満なら0）。時間帯の外へはみ出す時刻を決めないので、
/// 短い時間帯（例 12:00〜12:15）でも時間帯内に出せ、決めた時刻は時間帯が終わるまで有効なまま
/// （判定のたびに決め直して保存し続けない）。日付をまたぐ時間帯は、その時間帯の終わり（翌日側）までを残りとする。
///
/// 保存済みの時刻が無効（決め直す）になるのは次のとき。
/// - 未設定・壊れている（初回・旧データ）。
/// - 前回通知時刻以前（その時刻で通知済み＝使い終わった。次はクールタイム明けの最初の判定で決め直すので、
///   間隔は「クールタイム（閉じる/無視後の抑制を含む）＋ランダムな待ち」になる）。
/// - 今いる時間帯（その回）の外。前日に決めた時刻・午前の回に決めた時刻は引き継がず、
///   その回の最初の判定から決め直す（時間帯の開始直後に通知がそろわないようにする）。
fn random_timing_in<Tz: TimeZone>(
    tz: &Tz,
    state: &mut PersistedYuukoState,
    now: DateTime<Utc>,
    time_ranges: &[(&str, &str)],
    seed: u64,
) -> (ModeTimingGate, bool) {
    // 呼び出し側（can_notify）で時間帯内を確認済み。万一外なら決め直さずに待つ（出さない側）。
    let Some((range_start, range_end)) = current_range_occurrence(tz, now, time_ranges) else {
        return (ModeTimingGate::Waiting, false);
    };
    let last_notified = state.last_notified_at.as_deref().and_then(parse_timestamp);
    let scheduled = state
        .next_random_notify_at
        .as_deref()
        .and_then(parse_timestamp)
        .filter(|at| last_notified.is_none_or(|last| *at > last))
        .filter(|at| range_start <= *at && *at <= range_end);

    if let Some(at) = scheduled {
        let gate = if now >= at {
            ModeTimingGate::Allowed
        } else {
            ModeTimingGate::Waiting
        };
        return (gate, false);
    }

    // 時間帯の残りから判定間隔を引いた分までに抑える（終わり間際に決めて出しそびれないため。
    // POLL_INTERVAL_MINUTES を参照）。残りが判定間隔以下なら待ち0分（今すぐ出す）。
    let remaining_minutes = (range_end - now).num_minutes();
    let max_minutes = RANDOM_DELAY_MAX_MINUTES
        .min(remaining_minutes - POLL_INTERVAL_MINUTES)
        .max(0);
    let at = now + random_delay_from_seed(seed, max_minutes);
    state.next_random_notify_at = Some(format_timestamp(at));
    let gate = if at <= now {
        ModeTimingGate::Allowed
    } else {
        ModeTimingGate::Waiting
    };
    (gate, true)
}

/// 今を含む時間帯の「回」（開始・終了の時刻）を返す。重なる時間帯があれば終わりが遅い方。
/// 終了は既存の時間帯判定と同じく終了時刻の分を含むが、ここでは終了時刻ちょうど（秒0）を終わりとする。
/// 日付をまたぐ時間帯（例 22:00〜02:00）は開始時刻からの経過分を 24 時間で折り返して数える。
fn current_range_occurrence<Tz: TimeZone>(
    tz: &Tz,
    now: DateTime<Utc>,
    time_ranges: &[(&str, &str)],
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let (current_minutes, minute_floor) = local_minute_and_floor(tz, now);
    time_ranges
        .iter()
        .filter_map(|(start, end)| {
            let (start_minutes, range_length) = range_minutes(start, end)?;
            let elapsed = (current_minutes - start_minutes).rem_euclid(MINUTES_PER_DAY);
            if elapsed > range_length {
                return None;
            }
            let occurrence_start = minute_floor - Duration::minutes(elapsed);
            Some((
                occurrence_start,
                occurrence_start + Duration::minutes(range_length),
            ))
        })
        .max_by_key(|(_, end)| *end)
}

/// 時間帯の開始（0時からの分）と長さ（分）。不正な時刻・長さ0は None（既存の時間帯判定でも無効扱い）。
fn range_minutes(start: &str, end: &str) -> Option<(i64, i64)> {
    let start = parse_hour_minute(start)?;
    let end = parse_hour_minute(end)?;
    let start_minutes = i64::from(start.0 * 60 + start.1);
    let end_minutes = i64::from(end.0 * 60 + end.1);
    let range_length = (end_minutes - start_minutes).rem_euclid(MINUTES_PER_DAY);
    (range_length > 0).then_some((start_minutes, range_length))
}

/// 今のローカル時刻（0時からの分）と、秒以下を切り捨てた今（UTC）。
///
/// 回の開始は「今から経過分を引く」で求めるため、夏時間の切り替えをまたぐ回では開始・終了が
/// 切り替え幅だけずれうる（日本・JST には夏時間が無いため実運用では起きない）。ずれても
/// その回を1回多く/少なく扱うだけで、日次上限・クールタイムは別に効くため通知が過剰にはならない。
fn local_minute_and_floor<Tz: TimeZone>(tz: &Tz, now: DateTime<Utc>) -> (i64, DateTime<Utc>) {
    let local = now.with_timezone(tz);
    let current_minutes = i64::from(local.hour() * 60 + local.minute());
    // 保存時刻は秒精度のため、比較をそろえるよう秒以下を切り捨てる。
    let minute_floor = now
        - Duration::seconds(i64::from(local.second()))
        - Duration::nanoseconds(i64::from(local.nanosecond()));
    (current_minutes, minute_floor)
}

/// 定時モード: 今が「いずれかの時間帯の開始時刻から FIXED_TIME_WINDOW_MINUTES 分以内（時間帯が短ければその長さまで）」で、
/// その回の開始時刻以降にまだ通知していなければ出してよい。1つの時間帯につき1日1回。
/// 日付をまたぐ時間帯（例 22:00〜02:00）は開始時刻からの経過分を 24 時間で折り返して数える。
fn fixed_time_timing_in<Tz: TimeZone>(
    tz: &Tz,
    state: &PersistedYuukoState,
    now: DateTime<Utc>,
    time_ranges: &[(&str, &str)],
) -> ModeTimingGate {
    let last_notified = state.last_notified_at.as_deref().and_then(parse_timestamp);
    // 回の開始は今から経過分を引いて求める（夏時間の扱いは local_minute_and_floor を参照）。
    let (current_minutes, minute_floor) = local_minute_and_floor(tz, now);

    for (start, end) in time_ranges {
        let Some((start_minutes, range_length)) = range_minutes(start, end) else {
            continue;
        };
        let window = FIXED_TIME_WINDOW_MINUTES.min(range_length);
        let elapsed = (current_minutes - start_minutes).rem_euclid(MINUTES_PER_DAY);
        if elapsed >= window {
            continue;
        }
        let slot_start = minute_floor - Duration::minutes(elapsed);
        if last_notified.is_none_or(|last| last < slot_start) {
            return ModeTimingGate::Allowed;
        }
    }
    ModeTimingGate::OutsideSlot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::yuuko::is_within_any_notification_time_range;
    use chrono::FixedOffset;

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn jst_at(d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        jst()
            .with_ymd_and_hms(2026, 10, d, h, mi, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    const WORK: [(&str, &str); 2] = [("09:00", "12:00"), ("13:00", "18:00")];

    /// 指定した待ち（分）になるシードを探す（シードの写像に依存せずテストを固定する）。
    /// 上限は既定の90分（時間帯の残りが90分以上あるとき）。
    fn seed_for_delay(minutes: i64) -> u64 {
        seed_for_delay_within(minutes, RANDOM_DELAY_MAX_MINUTES)
    }

    fn seed_for_delay_within(minutes: i64, max_minutes: i64) -> u64 {
        (0..100_000u64)
            .find(|seed| random_delay_from_seed(*seed, max_minutes) == Duration::minutes(minutes))
            .expect("seed for delay")
    }

    fn check(
        state: &mut PersistedYuukoState,
        mode: NotificationMode,
        now: DateTime<Utc>,
        ranges: &[(&str, &str)],
        seed: u64,
    ) -> (ModeTimingGate, bool) {
        let ranges = mode.time_ranges(ranges);
        check_mode_timing_in(&jst(), state, mode, now, &ranges, seed)
    }

    #[test]
    fn mode_is_parsed_from_storage_and_unknown_falls_back_to_work_time_random() {
        assert_eq!(
            NotificationMode::from_storage("random"),
            NotificationMode::Random
        );
        assert_eq!(
            NotificationMode::from_storage("random_in_work_time"),
            NotificationMode::RandomInWorkTime
        );
        assert_eq!(
            NotificationMode::from_storage("fixed_time"),
            NotificationMode::FixedTime
        );
        assert_eq!(
            NotificationMode::from_storage("none"),
            NotificationMode::Off
        );
        assert_eq!(
            NotificationMode::from_storage("unknown"),
            NotificationMode::RandomInWorkTime
        );
        assert_eq!(
            NotificationMode::from_storage(""),
            NotificationMode::RandomInWorkTime
        );
    }

    #[test]
    fn random_mode_ignores_work_ranges_but_work_time_random_keeps_them() {
        assert_eq!(
            NotificationMode::Random.time_ranges(&WORK),
            vec![RANDOM_MODE_RANGE]
        );
        assert_eq!(
            NotificationMode::RandomInWorkTime.time_ranges(&WORK),
            WORK.to_vec()
        );
        assert_eq!(
            NotificationMode::FixedTime.time_ranges(&WORK),
            WORK.to_vec()
        );
    }

    #[test]
    fn random_delay_stays_within_bounds_and_varies() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..2000u64 {
            let minutes = random_delay_from_seed(seed, RANDOM_DELAY_MAX_MINUTES).num_minutes();
            assert!((0..=RANDOM_DELAY_MAX_MINUTES).contains(&minutes));
            seen.insert(minutes);
            // 上限を縮めても範囲に収まる。
            let short = random_delay_from_seed(seed, 15).num_minutes();
            assert!((0..=15).contains(&short));
        }
        assert!(seen.len() > 80, "delays should vary: {}", seen.len());
        // 待ち0分のシードは上限に関係なく0分。
        let zero = seed_for_delay(0);
        for max in [0, 1, 15, 90] {
            assert_eq!(random_delay_from_seed(zero, max), Duration::zero());
        }
    }

    #[test]
    fn short_range_delay_is_capped_by_remaining_minutes_and_fires_inside() {
        let short = [("12:00", "12:15")];
        // 12:00 の判定では残り15分。判定間隔5分を引いた 0〜10 分に収まる（最大でも 12:10）。
        for seed in 0..500u64 {
            let mut state = PersistedYuukoState::default();
            let (gate, changed) = check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(19, 12, 0),
                &short,
                seed,
            );
            assert!(changed);
            let at = parse_timestamp(state.next_random_notify_at.as_deref().unwrap()).unwrap();
            assert!(
                at >= jst_at(19, 12, 0) && at <= jst_at(19, 12, 10),
                "seed={seed}"
            );
            if gate == ModeTimingGate::Waiting {
                // 決めた時刻は時間帯の終わりまで有効で、次の判定で決め直さない。その時刻になれば出せる。
                let saved = state.next_random_notify_at.clone();
                let (_, changed) = check(
                    &mut state,
                    NotificationMode::RandomInWorkTime,
                    jst_at(19, 12, 5),
                    &short,
                    1,
                );
                assert!(!changed);
                assert_eq!(state.next_random_notify_at, saved);
                assert_eq!(
                    check(
                        &mut state,
                        NotificationMode::RandomInWorkTime,
                        at,
                        &short,
                        1
                    ),
                    (ModeTimingGate::Allowed, false)
                );
            }
        }

        // 残りが上限より短いときの待ちは「残り − 判定間隔」を上限にした値になる。
        let mut state = PersistedYuukoState::default();
        check(
            &mut state,
            NotificationMode::RandomInWorkTime,
            jst_at(19, 12, 0),
            &short,
            seed_for_delay_within(10, 10),
        );
        assert_eq!(
            state.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(19, 12, 10)).as_str())
        );

        // 残りが判定間隔以下（12:12）なら待たずに出す。
        let mut state = PersistedYuukoState::default();
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(19, 12, 12),
                &short,
                seed_for_delay(90)
            ),
            (ModeTimingGate::Allowed, true)
        );
    }

    #[test]
    fn cross_midnight_range_keeps_the_schedule_across_the_date_change() {
        let overnight = [("22:00", "02:00")];
        let mut state = PersistedYuukoState::default();
        // 23:50 に決めた 00:40（翌日側）は、同じ回の中なので日付が変わる前も後も決め直さない。
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(19, 23, 50),
                &overnight,
                seed_for_delay(50)
            ),
            (ModeTimingGate::Waiting, true)
        );
        let saved = state.next_random_notify_at.clone();
        assert_eq!(
            saved.as_deref(),
            Some(format_timestamp(jst_at(20, 0, 40)).as_str())
        );
        for now in [jst_at(19, 23, 55), jst_at(20, 0, 0), jst_at(20, 0, 30)] {
            assert_eq!(
                check(
                    &mut state,
                    NotificationMode::RandomInWorkTime,
                    now,
                    &overnight,
                    1
                ),
                (ModeTimingGate::Waiting, false)
            );
            assert_eq!(state.next_random_notify_at, saved);
        }
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(20, 0, 40),
                &overnight,
                1
            ),
            (ModeTimingGate::Allowed, false)
        );

        // 01:30 に決めるときは、回の終わり（02:00）までの30分から判定間隔を引いた25分が上限。
        let mut late = PersistedYuukoState::default();
        check(
            &mut late,
            NotificationMode::RandomInWorkTime,
            jst_at(20, 1, 30),
            &overnight,
            seed_for_delay_within(25, 25),
        );
        assert_eq!(
            late.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(20, 1, 55)).as_str())
        );
    }

    #[test]
    fn random_mode_delay_is_capped_at_twenty_three() {
        let mut state = PersistedYuukoState::default();
        check(
            &mut state,
            NotificationMode::Random,
            jst_at(19, 22, 40),
            &WORK,
            seed_for_delay_within(15, 15),
        );
        // 23:00 までの残り20分から判定間隔5分を引いた 22:55 が最も遅い時刻。
        assert_eq!(
            state.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(19, 22, 55)).as_str())
        );
    }

    #[test]
    fn random_mode_waits_a_random_delay_then_allows_and_persists_the_time() {
        let mut state = PersistedYuukoState::default();
        let seed = seed_for_delay(37);
        let first = jst_at(19, 9, 0);

        // 時間帯の最初の判定では出さず、37分後を「次に通知してよい時刻」として保存する。
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                first,
                &WORK,
                seed
            ),
            (ModeTimingGate::Waiting, true)
        );
        assert_eq!(
            state.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(19, 9, 37)).as_str())
        );

        // 待ちの途中は（別のシードでも）決め直さない。保存済みの時刻なので再起動しても続きから待つ。
        let restored: PersistedYuukoState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        let mut state = restored;
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(19, 9, 35),
                &WORK,
                1
            ),
            (ModeTimingGate::Waiting, false)
        );
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(19, 9, 40),
                &WORK,
                1
            ),
            (ModeTimingGate::Allowed, false)
        );
    }

    #[test]
    fn zero_delay_allows_immediately() {
        let mut state = PersistedYuukoState::default();
        assert_eq!(
            check(
                &mut state,
                NotificationMode::Random,
                jst_at(19, 9, 0),
                &WORK,
                seed_for_delay(0)
            ),
            (ModeTimingGate::Allowed, true)
        );
    }

    #[test]
    fn schedule_is_rerolled_after_each_notification() {
        let mut state = PersistedYuukoState {
            next_random_notify_at: Some(format_timestamp(jst_at(19, 9, 20))),
            // 9:20 に通知済み（使い終わった時刻）。
            last_notified_at: Some(format_timestamp(jst_at(19, 9, 20))),
            ..PersistedYuukoState::default()
        };
        // クールタイム明け（10:20）の最初の判定で、そこから改めてランダムに待つ。
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(19, 10, 20),
                &WORK,
                seed_for_delay(55)
            ),
            (ModeTimingGate::Waiting, true)
        );
        assert_eq!(
            state.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(19, 11, 15)).as_str())
        );
    }

    #[test]
    fn schedule_from_previous_day_is_not_carried_over() {
        let mut state = PersistedYuukoState {
            // 前日の夕方に決めたまま PC を閉じた時刻。
            next_random_notify_at: Some(format_timestamp(jst_at(18, 17, 30))),
            ..PersistedYuukoState::default()
        };
        // 翌朝 9:00 にすぐ出さず、その日の最初の判定から決め直す。
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(19, 9, 0),
                &WORK,
                seed_for_delay(20)
            ),
            (ModeTimingGate::Waiting, true)
        );
        assert_eq!(
            state.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(19, 9, 20)).as_str())
        );
    }

    #[test]
    fn schedule_falling_into_lunch_break_is_rerolled_in_the_afternoon_range() {
        let mut state = PersistedYuukoState {
            // 11:30 に決めた待ちが昼休み（12:20）にかかった。
            next_random_notify_at: Some(format_timestamp(jst_at(19, 12, 20))),
            ..PersistedYuukoState::default()
        };
        // 午後の最初の判定（13:00）でちょうど出さず、そこから決め直す。
        assert_eq!(
            check(
                &mut state,
                NotificationMode::RandomInWorkTime,
                jst_at(19, 13, 0),
                &WORK,
                seed_for_delay(45)
            ),
            (ModeTimingGate::Waiting, true)
        );
        assert_eq!(
            state.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(19, 13, 45)).as_str())
        );

        // ランダム（7:00〜23:00）なら昼休みの時刻もそのまま有効。
        let mut state = PersistedYuukoState {
            next_random_notify_at: Some(format_timestamp(jst_at(19, 12, 20))),
            ..PersistedYuukoState::default()
        };
        assert_eq!(
            check(
                &mut state,
                NotificationMode::Random,
                jst_at(19, 13, 0),
                &WORK,
                1
            ),
            (ModeTimingGate::Allowed, false)
        );
    }

    #[test]
    fn broken_saved_time_is_rerolled() {
        let mut state = PersistedYuukoState {
            next_random_notify_at: Some("not-a-time".to_string()),
            ..PersistedYuukoState::default()
        };
        let (gate, changed) = check(
            &mut state,
            NotificationMode::Random,
            jst_at(19, 9, 0),
            &WORK,
            seed_for_delay(10),
        );
        assert_eq!((gate, changed), (ModeTimingGate::Waiting, true));
        assert_eq!(
            state.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(19, 9, 10)).as_str())
        );
    }

    #[test]
    fn fixed_time_allows_once_per_range_start_within_the_window() {
        let mut state = PersistedYuukoState::default();
        let mode = NotificationMode::FixedTime;

        // 開始前・開始時刻・猶予の終わり。
        assert_eq!(
            check(&mut state, mode, jst_at(19, 8, 55), &WORK, 0).0,
            ModeTimingGate::OutsideSlot
        );
        assert_eq!(
            check(&mut state, mode, jst_at(19, 9, 0), &WORK, 0).0,
            ModeTimingGate::Allowed
        );
        assert_eq!(
            check(&mut state, mode, jst_at(19, 9, 59), &WORK, 0).0,
            ModeTimingGate::Allowed
        );
        // 開始から60分を過ぎたら、まだ出していなくてもその回は見送る。
        assert_eq!(
            check(&mut state, mode, jst_at(19, 10, 0), &WORK, 0).0,
            ModeTimingGate::OutsideSlot
        );

        // 9:05 に出したら、同じ回ではもう出さない。午後の回（13:00）は別に出せる。
        state.last_notified_at = Some(format_timestamp(jst_at(19, 9, 5)));
        assert_eq!(
            check(&mut state, mode, jst_at(19, 9, 10), &WORK, 0).0,
            ModeTimingGate::OutsideSlot
        );
        assert_eq!(
            check(&mut state, mode, jst_at(19, 13, 0), &WORK, 0).0,
            ModeTimingGate::Allowed
        );

        // 翌日の 9:00 の回は前日の通知に関係なく出せる。
        state.last_notified_at = Some(format_timestamp(jst_at(19, 13, 0)));
        assert_eq!(
            check(&mut state, mode, jst_at(20, 9, 0), &WORK, 0).0,
            ModeTimingGate::Allowed
        );
        // 定時はランダムの時刻を使わない。
        assert!(state.next_random_notify_at.is_none());
    }

    #[test]
    fn fixed_time_window_is_limited_by_short_ranges_and_handles_overnight_ranges() {
        let mut state = PersistedYuukoState::default();
        let mode = NotificationMode::FixedTime;

        // 30分しかない時間帯は、その長さまで。
        let short = [("15:00", "15:30")];
        assert_eq!(
            check(&mut state, mode, jst_at(19, 15, 25), &short, 0).0,
            ModeTimingGate::Allowed
        );
        assert_eq!(
            check(&mut state, mode, jst_at(19, 15, 30), &short, 0).0,
            ModeTimingGate::OutsideSlot
        );

        // 23:30〜02:00 の回は日付をまたいだ 00:10 でも開始から40分なので出せる。
        let overnight = [("23:30", "02:00")];
        assert_eq!(
            check(&mut state, mode, jst_at(20, 0, 10), &overnight, 0).0,
            ModeTimingGate::Allowed
        );
        // 前日 23:40 に出していれば、その回は使用済み。
        state.last_notified_at = Some(format_timestamp(jst_at(19, 23, 40)));
        assert_eq!(
            check(&mut state, mode, jst_at(20, 0, 10), &overnight, 0).0,
            ModeTimingGate::OutsideSlot
        );

        // 長さ0・不正な時間帯は無視する。
        let invalid = [("10:00", "10:00"), ("25:00", "26:00")];
        let mut fresh = PersistedYuukoState::default();
        assert_eq!(
            check(&mut fresh, mode, jst_at(19, 10, 0), &invalid, 0).0,
            ModeTimingGate::OutsideSlot
        );
    }

    #[test]
    fn storage_values_round_trip_and_unknown_values_are_rejected_for_saving() {
        for mode in [
            NotificationMode::RandomInWorkTime,
            NotificationMode::Random,
            NotificationMode::FixedTime,
            NotificationMode::Off,
        ] {
            assert_eq!(
                NotificationMode::parse_storage(mode.as_storage()),
                Some(mode)
            );
            assert_eq!(NotificationMode::from_storage(mode.as_storage()), mode);
        }
        assert_eq!(NotificationMode::parse_storage("unknown"), None);
        assert_eq!(NotificationMode::parse_storage(""), None);
    }

    #[test]
    fn random_mode_range_is_seven_to_twenty_three() {
        let ranges = NotificationMode::Random.time_ranges(&WORK);
        let within = |h: u32, m: u32| {
            is_within_any_notification_time_range(h * 60 + m, ranges.iter().copied())
        };
        assert!(!within(6, 59));
        assert!(within(7, 0));
        assert!(within(12, 30)); // 通知時間帯（昼休み）とは独立。
        assert!(within(23, 0));
        assert!(!within(23, 1));
        assert!(!within(2, 0));
    }

    #[test]
    fn random_mode_time_after_twenty_three_is_rerolled_next_morning() {
        let mut state = PersistedYuukoState {
            // 22:50 に決めた待ちが 23:40（範囲外）になった。
            next_random_notify_at: Some(format_timestamp(jst_at(19, 23, 40))),
            ..PersistedYuukoState::default()
        };
        // 翌朝 7:00 の最初の判定ですぐ出さず、そこから決め直す。
        assert_eq!(
            check(
                &mut state,
                NotificationMode::Random,
                jst_at(20, 7, 0),
                &WORK,
                seed_for_delay(30)
            ),
            (ModeTimingGate::Waiting, true)
        );
        assert_eq!(
            state.next_random_notify_at.as_deref(),
            Some(format_timestamp(jst_at(20, 7, 30)).as_str())
        );
    }

    #[test]
    fn off_mode_never_allows_here() {
        let mut state = PersistedYuukoState::default();
        assert_eq!(
            check(
                &mut state,
                NotificationMode::Off,
                jst_at(19, 9, 0),
                &WORK,
                0
            ),
            (ModeTimingGate::OutsideSlot, false)
        );
    }
}
