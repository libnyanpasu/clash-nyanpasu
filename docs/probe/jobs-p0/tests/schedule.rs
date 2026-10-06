use chrono::{TimeZone, Timelike, Utc};
use chrono_tz::{America::New_York, Asia::Shanghai, Tz};
use croner::parser::{CronParser, Seconds, Year};
use std::time::Duration;

fn parser() -> CronParser {
    CronParser::builder()
        .seconds(Seconds::Optional)
        .year(Year::Disallowed)
        .build()
}

#[test]
fn five_and_six_fields_have_explicit_seconds_and_exclude_now() {
    let now = Shanghai.with_ymd_and_hms(2026, 9, 7, 12, 0, 0).unwrap();
    let minute = parser().parse("*/5 * * * *").unwrap();
    let second = parser().parse("*/10 * * * * *").unwrap();
    assert_eq!(
        minute.find_next_occurrence(&now, false).unwrap(),
        now + chrono::Duration::minutes(5)
    );
    assert_eq!(
        second.find_next_occurrence(&now, false).unwrap(),
        now + chrono::Duration::seconds(10)
    );
    assert_eq!(
        minute
            .find_next_occurrence(&now, false)
            .unwrap()
            .with_timezone(&Utc)
            .hour(),
        4
    );
}

#[test]
fn invalid_ranges_year_field_and_unknown_timezone_are_rejected() {
    for pattern in [
        "bad",
        "60 * * * *",
        "0 24 * * *",
        "0 0 0 * * * 2026",
        "*/0 * * * *",
    ] {
        assert!(parser().parse(pattern).is_err(), "accepted {pattern}");
    }
    assert!("Invalid/Zone".parse::<Tz>().is_err());
}

#[test]
fn impossible_calendar_has_no_next_occurrence() {
    let cron = parser().parse("0 0 30 2 *").unwrap();
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    assert!(cron.find_next_occurrence(&now, false).is_err());
}

#[test]
fn fixed_time_gap_moves_to_first_valid_instant() {
    let cron = parser().parse("30 2 * * *").unwrap();
    let before = New_York.with_ymd_and_hms(2026, 3, 8, 1, 59, 59).unwrap();
    assert_eq!(
        cron.find_next_occurrence(&before, false).unwrap(),
        New_York.with_ymd_and_hms(2026, 3, 8, 3, 0, 0).unwrap()
    );
}

#[test]
fn fixed_time_overlap_runs_once_but_every_minute_covers_both_offsets() {
    let before = New_York.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();
    let local = New_York.with_ymd_and_hms(2026, 11, 1, 1, 30, 0);
    let first = local.earliest().unwrap();
    let second = local.latest().unwrap();
    assert_ne!(first, second);
    let fixed = parser().parse("30 1 * * *").unwrap();
    assert_eq!(fixed.find_next_occurrence(&before, false).unwrap(), first);
    assert_eq!(
        fixed.find_next_occurrence(&first, false).unwrap(),
        New_York.with_ymd_and_hms(2026, 11, 2, 1, 30, 0).unwrap()
    );
    let every_minute = parser().parse("* * * * *").unwrap();
    let occurrences: Vec<_> = every_minute
        .iter_after(first - chrono::Duration::minutes(1))
        .take(121)
        .collect();
    assert!(occurrences.contains(&first));
    assert!(occurrences.contains(&second));
    assert!(occurrences.windows(2).all(|pair| pair[0] < pair[1]));
}

#[tokio::test(start_paused = true)]
async fn interval_at_delays_first_tick_but_skip_still_delivers_one_overdue_tick() {
    use tokio::time::{Instant, MissedTickBehavior, advance, interval_at};
    let start = Instant::now();
    let mut timer = interval_at(start + Duration::from_secs(10), Duration::from_secs(10));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    assert_eq!(timer.tick().await, start + Duration::from_secs(10));
    advance(Duration::from_secs(35)).await;
    // Jobs must separately enforce its lateness policy, even with Tokio Skip.
    assert_eq!(timer.tick().await, start + Duration::from_secs(20));
    assert_eq!(timer.tick().await, start + Duration::from_secs(50));
}
