//! Parsing the adapter name Windows reports for an interface.

/// A GUID in its four-field layout, which `windows-sys` builds a `GUID` from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Guid {
    pub(super) data1: u32,
    pub(super) data2: u16,
    pub(super) data3: u16,
    pub(super) data4: [u8; 8],
}

/// Parses `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`, the form of a Windows
/// adapter name. Anything else is not a Wi-Fi adapter this module can query.
pub(super) fn parse(name: &str) -> Option<Guid> {
    let inner = name.strip_prefix('{')?.strip_suffix('}')?;
    let groups: Vec<&str> = inner.split('-').collect();
    let [first, second, third, fourth, rest] = groups.as_slice() else {
        return None;
    };
    let shape = [
        first.len(),
        second.len(),
        third.len(),
        fourth.len(),
        rest.len(),
    ];
    // `from_str_radix` also takes a sign, which no GUID has.
    if shape != [8, 4, 4, 4, 12]
        || !inner
            .bytes()
            .all(|byte| byte == b'-' || byte.is_ascii_hexdigit())
    {
        return None;
    }

    let tail = format!("{fourth}{rest}");
    let mut data4 = [0_u8; 8];
    for (index, byte) in data4.iter_mut().enumerate() {
        *byte = u8::from_str_radix(tail.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }

    Some(Guid {
        data1: u32::from_str_radix(first, 16).ok()?,
        data2: u16::from_str_radix(second, 16).ok()?,
        data3: u16::from_str_radix(third, 16).ok()?,
        data4,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_adapter_name_parses_into_guid_fields() {
        assert_eq!(
            parse("{1F2E3D4C-5B6A-7988-A1B2-C3D4E5F60718}"),
            Some(Guid {
                data1: 0x1F2E_3D4C,
                data2: 0x5B6A,
                data3: 0x7988,
                data4: [0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x07, 0x18],
            })
        );
    }

    #[test]
    fn a_name_that_is_not_a_braced_guid_is_rejected() {
        for name in [
            "wlan0",
            "1F2E3D4C-5B6A-7988-A1B2-C3D4E5F60718",
            "{1F2E3D4C-5B6A-7988-A1B2}",
            "{1F2E3D4C-5B6A-7988-A1B2-C3D4E5F6071}",
            "{1F2E3D4C-5B6A-7988-A1B2-C3D4E5F6071Z}",
            "{+F2E3D4C-5B6A-7988-A1B2-C3D4E5F60718}",
        ] {
            assert_eq!(parse(name), None, "{name}");
        }
    }
}
