use humansize::{DECIMAL, FormatSizeOptions, format_size};
use nyanpasu_widget_protocol::StatisticMessage;
use slint::SharedString;

/// A statistic as the widget shows it: decimal sizes with up to two decimals,
/// speeds with a `/s` suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatisticText {
    pub download_total: SharedString,
    pub upload_total: SharedString,
    pub download_speed: SharedString,
    pub upload_speed: SharedString,
}

impl From<&StatisticMessage> for StatisticText {
    fn from(statistic: &StatisticMessage) -> Self {
        Self {
            download_total: total(statistic.download_total),
            upload_total: total(statistic.upload_total),
            download_speed: speed(statistic.download_speed),
            upload_speed: speed(statistic.upload_speed),
        }
    }
}

fn total(bytes: u64) -> SharedString {
    size(bytes, DECIMAL)
}

fn speed(bytes_per_second: u64) -> SharedString {
    size(bytes_per_second, DECIMAL.suffix("/s"))
}

fn size(bytes: u64, options: FormatSizeOptions) -> SharedString {
    let text = format_size(bytes, options);
    // humansize picks the unit before rounding, so 999.995 kB reads
    // "1000.00 kB", wider than the value cells. Show it as the next unit.
    if text.starts_with("1000.") {
        let mut next_unit: u64 = 1;
        while next_unit <= bytes {
            next_unit = next_unit.saturating_mul(1000);
        }
        return format_size(next_unit, options).into();
    }
    text.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_decimal_totals_and_speeds() {
        let text = StatisticText::from(&StatisticMessage {
            download_total: 123_456_789,
            upload_total: 999,
            download_speed: 345_678,
            upload_speed: 0,
        });

        assert_eq!(text.download_total, "123.46 MB");
        assert_eq!(text.upload_total, "999 B");
        assert_eq!(text.download_speed, "345.68 kB/s");
        assert_eq!(text.upload_speed, "0 B/s");
    }

    /// The UI sizes its value cells for at most three integer and two
    /// fraction digits.
    #[test]
    fn values_never_reach_four_integer_digits() {
        assert_eq!(total(888_884_999), "888.88 MB");
        assert_eq!(speed(888_884_999), "888.88 MB/s");
        assert_eq!(speed(999_994_999), "999.99 MB/s");
        assert_eq!(speed(999_999), "1 MB/s");
        assert_eq!(total(999_999_999), "1 GB");
        assert_eq!(total(u64::MAX), "18.45 EB");
    }
}
