use crate::{Error, Result};
use iptools::iprange::{IpRange, IpVer};

/// Convert ipv4 cidr to wildcard
///
/// Example:
/// ```
/// use sysproxy::utils::ipv4_cidr_to_wildcard;
/// assert_eq!(ipv4_cidr_to_wildcard("127.0.0.1/8").unwrap(), vec!["127.*".to_string()]);
/// ```
pub fn ipv4_cidr_to_wildcard(cidr: &str) -> Result<Vec<String>> {
    let ip = IpRange::new(cidr, "").or(Err(Error::ParseStr(cidr.into())))?;

    if ip.get_version() != IpVer::IPV4 {
        return Err(Error::ParseStr(cidr.into()));
    }

    let (start, end) = ip.get_range().unwrap();
    let start = start.split('.').collect::<Vec<&str>>();
    let end = end.split('.').collect::<Vec<&str>>();

    let mut ret = vec![];
    let mut each = String::new();
    for i in 0..4 {
        if start[i] == end[i] {
            each.push_str(start[i]);
            if i != 3 {
                each.push('.');
            }
            continue;
        }

        if start[i] == "0" && end[i] == "255" {
            each.push('*');
            ret.push(each);
            break;
        }

        let s = start[i]
            .parse::<u16>()
            .or(Err(Error::ParseStr(cidr.into())))?;
        let e = end[i]
            .parse::<u16>()
            .or(Err(Error::ParseStr(cidr.into())))?;

        for j in s..e + 1 {
            let mut builder = each.clone();
            builder.push_str(&j.to_string());
            if i != 3 {
                builder.push_str(".*");
            }
            ret.push(builder);
        }
        break;
    }
    Ok(ret)
}

#[test]
fn test_ipv4_cidr_to_wildcard() {
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/1"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/2"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/3"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/4"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/5"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/6"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/7"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/8"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/9"));
    println!("{:?}", ipv4_cidr_to_wildcard("127.0.0.1/10"));
}
