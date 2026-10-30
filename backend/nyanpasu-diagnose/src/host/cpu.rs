//! The CPU's signature and the microcode it runs: a few CPUID instructions plus
//! at most one registry value or `/proc/cpuinfo` read.

use serde::Serialize;

/// Display family, model and stepping, from CPUID leaf 1 (x86 only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CpuSignature {
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(super) fn signature() -> Option<CpuSignature> {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::__cpuid;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::__cpuid;

    // Leaf 1 exists when the highest standard leaf is at least 1.
    (__cpuid(0).eax >= 1).then(|| decode_signature(__cpuid(1).eax))
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub(super) fn signature() -> Option<CpuSignature> {
    None
}

/// The microcode revision currently loaded, as the vendor numbers it.
#[cfg(windows)]
pub(super) fn microcode() -> Option<u32> {
    // Every logical processor runs the same revision; processor 0 always exists.
    let value = windows_registry::LOCAL_MACHINE
        .open(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
        .ok()?
        .get_value("Update Revision")
        .ok()?;
    decode_windows_update_revision(&value)
}

#[cfg(target_os = "linux")]
pub(super) fn microcode() -> Option<u32> {
    parse_proc_cpuinfo_microcode(&std::fs::read_to_string("/proc/cpuinfo").ok()?)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn microcode() -> Option<u32> {
    // Apple Silicon has no user-visible revision.
    None
}

/// Decodes CPUID leaf 1 EAX into the display family and model, following the
/// rules shared by the Intel SDM and AMD APM.
#[cfg_attr(
    not(any(target_arch = "x86", target_arch = "x86_64")),
    allow(dead_code)
)]
fn decode_signature(eax: u32) -> CpuSignature {
    let stepping = eax & 0xF;
    let base_model = (eax >> 4) & 0xF;
    let base_family = (eax >> 8) & 0xF;
    let extended_model = (eax >> 16) & 0xF;
    let extended_family = (eax >> 20) & 0xFF;

    let family = if base_family == 0xF {
        base_family + extended_family
    } else {
        base_family
    };
    let model = if base_family == 0x6 || base_family == 0xF {
        (extended_model << 4) | base_model
    } else {
        base_model
    };
    CpuSignature {
        family,
        model,
        stepping,
    }
}

/// `Update Revision` is the raw 64-bit revision MSR on older Windows builds:
/// Intel keeps the revision in the high dword and AMD in the low one, the other
/// half being zero. Windows 11 24H2 and later store just the 4-byte revision.
#[cfg(any(windows, test))]
fn decode_windows_update_revision(bytes: &[u8]) -> Option<u32> {
    let dword = |range: std::ops::Range<usize>| {
        Some(u32::from_le_bytes(bytes.get(range)?.try_into().ok()?))
    };
    let revision = match bytes.len() {
        4 => dword(0..4)?,
        8 => match dword(4..8)? {
            0 => dword(0..4)?,
            high => high,
        },
        _ => return None,
    };
    (revision != 0).then_some(revision)
}

/// Reads the `microcode : 0x…` line the kernel prints for each processor.
#[cfg(any(target_os = "linux", test))]
fn parse_proc_cpuinfo_microcode(cpuinfo: &str) -> Option<u32> {
    cpuinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim() != "microcode" {
            return None;
        }
        let value = value.trim();
        let digits = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))?;
        u32::from_str_radix(digits, 16).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_applies_extended_fields_only_where_defined() {
        // Raptor Lake i9-14900KF: CPUID.1.EAX = 0xB0671.
        assert_eq!(
            decode_signature(0x000B_0671),
            CpuSignature {
                family: 6,
                model: 183,
                stepping: 1
            }
        );
        // Zen 4 (family 0x19 = 0xF + 0xA): CPUID.1.EAX = 0xA60F12.
        assert_eq!(
            decode_signature(0x00A6_0F12),
            CpuSignature {
                family: 25,
                model: 97,
                stepping: 2
            }
        );
        // Family 5 ignores the extended model bits.
        assert_eq!(decode_signature(0x0001_0543).model, 4);
    }

    #[test]
    fn windows_update_revision_picks_the_vendor_half() {
        // Windows 11 24H2+: the bare revision.
        assert_eq!(
            decode_windows_update_revision(&[0x2f, 0x01, 0x00, 0x00]),
            Some(0x12F)
        );
        // Intel MSR layout: revision in the high dword.
        assert_eq!(
            decode_windows_update_revision(&[0, 0, 0, 0, 0xf0, 0, 0, 0]),
            Some(0xF0)
        );
        // AMD MSR layout: patch level in the low dword.
        assert_eq!(
            decode_windows_update_revision(&[0x16, 0x10, 0x20, 0x0a, 0, 0, 0, 0]),
            Some(0x0A20_1016)
        );
        assert_eq!(decode_windows_update_revision(&[0; 8]), None);
        assert_eq!(decode_windows_update_revision(&[1, 2]), None);
    }

    #[test]
    fn proc_cpuinfo_microcode_is_hex() {
        let cpuinfo = "processor\t: 0\nmodel name\t: CPU\nmicrocode\t: 0x129\n\nprocessor\t: 1\nmicrocode\t: 0x129\n";
        assert_eq!(parse_proc_cpuinfo_microcode(cpuinfo), Some(0x129));
        assert_eq!(parse_proc_cpuinfo_microcode("model name\t: CPU\n"), None);
        assert_eq!(parse_proc_cpuinfo_microcode("microcode\t: n/a\n"), None);
    }
}
