//! Time buckets of the usage tables. Both tiers are counted from the Unix epoch in UTC.
use crate::model::{Tier, TrafficRange};

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// How many minute buckets before the current one the minute tier keeps: the span of the longest
/// range it answers.
const MINUTE_TIER_BUCKETS: u32 = 360;

/// Wall clock times before the epoch fall into bucket 0.
fn bucket(wall_ms: i64, size_ms: i64) -> u32 {
    u32::try_from(wall_ms.div_euclid(size_ms).max(0)).unwrap_or(u32::MAX)
}

pub fn minute_of(wall_ms: i64) -> u32 {
    bucket(wall_ms, MINUTE_MS)
}

pub fn hour_of(wall_ms: i64) -> u32 {
    bucket(wall_ms, HOUR_MS)
}

/// The oldest minute bucket worth keeping at `now_ms`; everything before it is out of the minute
/// tier's reach.
pub fn minute_cutoff(now_ms: i64) -> u32 {
    minute_of(now_ms).saturating_sub(MINUTE_TIER_BUCKETS)
}

impl TrafficRange {
    /// Up to six hours the minute tier answers; beyond that only the hour tier reaches back.
    pub fn tier(self) -> Tier {
        match self {
            TrafficRange::LastHour | TrafficRange::Last6Hours => Tier::Minute,
            TrafficRange::Last24Hours
            | TrafficRange::Last7Days
            | TrafficRange::Last30Days
            | TrafficRange::All => Tier::Hour,
        }
    }

    /// The first bucket of `self.tier()` the range covers, aligned down, so a range spans up to
    /// one bucket more than its nominal length. `None` reaches back to the beginning.
    pub fn start(self, now_ms: i64) -> Option<u32> {
        let span_ms = match self {
            TrafficRange::LastHour => HOUR_MS,
            TrafficRange::Last6Hours => 6 * HOUR_MS,
            TrafficRange::Last24Hours => 24 * HOUR_MS,
            TrafficRange::Last7Days => 7 * 24 * HOUR_MS,
            TrafficRange::Last30Days => 30 * 24 * HOUR_MS,
            TrafficRange::All => return None,
        };
        let from_ms = now_ms.saturating_sub(span_ms);
        Some(match self.tier() {
            Tier::Minute => minute_of(from_ms),
            Tier::Hour => hour_of(from_ms),
        })
    }
}

/// The one country a GeoIP tag list names. Databases mix country codes with categories such as
/// `google` or `private`, so only two-letter codes count. Several distinct countries are
/// ambiguous, and counting the bytes in each would duplicate them, so they are unknown just like
/// no country at all.
pub fn normalize_region<S: AsRef<str>>(codes: impl IntoIterator<Item = S>) -> String {
    let mut regions: Vec<String> = Vec::new();
    for code in codes {
        let code = code.as_ref();
        if code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            continue;
        }
        let code = code.to_ascii_uppercase();
        if !regions.contains(&code) {
            regions.push(code);
        }
    }
    match <[String; 1]>::try_from(regions) {
        Ok([region]) => region,
        _ => "unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_flip_exactly_on_the_boundary() {
        assert_eq!(minute_of(59_999), 0);
        assert_eq!(minute_of(60_000), 1);
        assert_eq!(minute_of(60_001), 1);
        assert_eq!(hour_of(HOUR_MS - 1), 0);
        assert_eq!(hour_of(HOUR_MS), 1);
        assert_eq!(hour_of(HOUR_MS + 1), 1);
    }

    #[test]
    fn times_before_the_epoch_fall_into_the_first_bucket() {
        assert_eq!(minute_of(-1), 0);
        assert_eq!(hour_of(i64::MIN), 0);
        assert_eq!(minute_of(i64::MAX), u32::MAX);
    }

    #[test]
    fn ranges_pick_the_tier_that_reaches_back_far_enough() {
        use TrafficRange::*;
        let tiers = [
            LastHour,
            Last6Hours,
            Last24Hours,
            Last7Days,
            Last30Days,
            All,
        ]
        .map(|r| r.tier());
        assert_eq!(
            tiers,
            [
                Tier::Minute,
                Tier::Minute,
                Tier::Hour,
                Tier::Hour,
                Tier::Hour,
                Tier::Hour
            ]
        );
    }

    #[test]
    fn the_minute_cutoff_is_where_the_longest_minute_range_starts() {
        let now = 1000 * HOUR_MS + 30 * MINUTE_MS + 17;
        assert_eq!(
            Some(minute_cutoff(now)),
            TrafficRange::Last6Hours.start(now)
        );
        assert_eq!(minute_cutoff(0), 0);
    }

    #[test]
    fn range_starts_align_down_to_the_bucket() {
        // 1000 hours and 30 minutes after the epoch.
        let now = 1000 * HOUR_MS + 30 * MINUTE_MS;
        let start = |range: TrafficRange| range.start(now);
        assert_eq!(
            start(TrafficRange::LastHour),
            Some(minute_of(999 * HOUR_MS + 30 * MINUTE_MS))
        );
        assert_eq!(
            start(TrafficRange::Last6Hours),
            Some(minute_of(994 * HOUR_MS + 30 * MINUTE_MS))
        );
        // The hour bucket that contains now - span is included whole.
        assert_eq!(start(TrafficRange::Last24Hours), Some(976));
        assert_eq!(start(TrafficRange::Last7Days), Some(832));
        assert_eq!(start(TrafficRange::Last30Days), Some(280));
        assert_eq!(start(TrafficRange::All), None);
    }

    #[test]
    fn a_range_starts_on_the_boundary_bucket_just_after_it() {
        let top_of_hour = 10 * HOUR_MS;
        // Exactly 1h ago is the start of its own minute bucket; 1 ms later still is.
        assert_eq!(
            TrafficRange::LastHour.start(top_of_hour + HOUR_MS),
            Some(minute_of(top_of_hour))
        );
        assert_eq!(
            TrafficRange::LastHour.start(top_of_hour + HOUR_MS + 1),
            Some(minute_of(top_of_hour))
        );
        assert_eq!(
            TrafficRange::LastHour.start(top_of_hour + HOUR_MS + MINUTE_MS),
            Some(minute_of(top_of_hour) + 1)
        );
    }

    #[test]
    fn ranges_reaching_before_the_epoch_start_at_zero() {
        assert_eq!(TrafficRange::LastHour.start(0), Some(0));
        assert_eq!(TrafficRange::Last30Days.start(i64::MIN), Some(0));
    }

    #[test]
    fn regions_are_normalized() {
        assert_eq!(normalize_region(Vec::<String>::new()), "unknown");
        assert_eq!(normalize_region(["cn"]), "CN");
        assert_eq!(normalize_region(["cn", "CN", "Cn"]), "CN");
        assert_eq!(normalize_region(["cn", "us"]), "unknown");
        assert_eq!(normalize_region([""]), "unknown");
    }

    #[test]
    fn a_country_among_category_tags_is_the_region() {
        assert_eq!(normalize_region(["google", "us"]), "US");
        assert_eq!(normalize_region(["telegram", "GB", "gb"]), "GB");
    }

    #[test]
    fn category_tags_alone_name_no_region() {
        assert_eq!(normalize_region(["private"]), "unknown");
        assert_eq!(normalize_region(["GOOGLE", "cloudflare"]), "unknown");
        assert_eq!(normalize_region(["c1"]), "unknown");
    }
}
