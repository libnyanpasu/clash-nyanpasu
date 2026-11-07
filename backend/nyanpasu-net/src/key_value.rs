//! Parsing semicolon-separated key-value fields.

use std::str::FromStr;

/// parse the string
/// xxx=123123; => 123123
pub fn parse_str<T: FromStr>(target: &str, key: &str) -> Option<T> {
    target.split(';').map(str::trim).find_map(|s| {
        let mut parts = s.splitn(2, '=');
        match (parts.next(), parts.next()) {
            (Some(k), Some(v)) if k == key => v.parse::<T>().ok(),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_value() {
        let test_1 = "upload=111; download=2222; total=3333; expire=444";
        let test_2 = "attachment; filename=Clash.yaml";

        assert_eq!(parse_str::<usize>(test_1, "upload").unwrap(), 111);
        assert_eq!(parse_str::<usize>(test_1, "download").unwrap(), 2222);
        assert_eq!(parse_str::<usize>(test_1, "total").unwrap(), 3333);
        assert_eq!(parse_str::<usize>(test_1, "expire").unwrap(), 444);
        assert_eq!(
            parse_str::<String>(test_2, "filename").unwrap(),
            format!("Clash.yaml")
        );

        assert_eq!(parse_str::<usize>(test_1, "aaa"), None);
        assert_eq!(parse_str::<usize>(test_1, "upload1"), None);
        assert_eq!(parse_str::<usize>(test_1, "expire1"), None);
        assert_eq!(parse_str::<usize>(test_2, "attachment"), None);
    }
}
