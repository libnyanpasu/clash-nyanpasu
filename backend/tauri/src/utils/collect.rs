use std::{borrow::Cow, collections::HashMap};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::consts::{BuildInfo, BUILD_INFO};
use humansize::{SizeFormatter, BINARY};
use nyanpasu_utils::core::{ClashCoreType, CoreType};
use serde::Serialize;
use sysinfo::System;

#[derive(Debug, Serialize, specta::Type)]
pub struct DeviceInfo<'a> {
    /// Device name, such as "Intel Core i5-8250U CPU @ 1.60GHz x 8"
    pub cpu: Vec<Cow<'a, str>>,
    /// GPU name, such as "Intel UHD Graphics 620 (Kabylake GT2)"
    // pub gpu: Cow<'a, str>,
    /// Memory size in bytes
    pub memory: Cow<'a, str>,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct EnvInfo<'a> {
    pub os: Cow<'a, str>,
    pub arch: Cow<'a, str>,
    pub core: CoreInfo<'a>,
    pub device: DeviceInfo<'a>,
    pub build_info: Cow<'a, BuildInfo>,
    // TODO: add service info
    // pub service_info: xxx
}

pub type CoreInfo<'a> = HashMap<Cow<'a, str>, Cow<'a, str>>;

pub fn collect_envs<'a>() -> Result<EnvInfo<'a>, std::io::Error> {
    let mut system = sysinfo::System::new_all();
    system.refresh_all();

    let device = DeviceInfo {
        cpu: {
            let mut cpus: Vec<(u64, &str, i32)> = Vec::new();
            for cpu in system.cpus().iter() {
                let item = cpus.iter_mut().find(|(_, name, _)| name == &cpu.brand());
                match item {
                    Some((_, _, count)) => *count += 1,
                    None => cpus.push((cpu.frequency(), cpu.brand(), 1)),
                }
            }
            cpus.iter()
                .map(|(freq, name, count)| {
                    Cow::Owned(format!(
                        "{} @ {:.2}GHz x {}",
                        name,
                        *freq as f64 / 1000.0,
                        count
                    ))
                })
                .collect()
        },
        memory: Cow::Owned(SizeFormatter::new(system.total_memory(), BINARY).to_string()),
    };

    let mut core = HashMap::new();
    for c in CoreType::get_supported_cores() {
        let name: &str = c.as_ref();

        let mut command = std::process::Command::new(
            super::dirs::get_data_or_sidecar_path(name)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?,
        );
        command.args(if matches!(c, CoreType::Clash(ClashCoreType::ClashRust)) {
            ["-V"]
        } else {
            ["-v"]
        });
        #[cfg(windows)]
        let command = command.creation_flags(0x08000000);
        let output = command.output().expect("failed to execute sidecar command");
        let stdout = String::from_utf8_lossy(&output.stdout);
        core.insert(
            Cow::Borrowed(name),
            Cow::Owned(stdout.replace("\n\n", " ").trim().to_owned()),
        );
    }
    Ok(EnvInfo {
        os: Cow::Owned(
            format!(
                "{} {}",
                System::long_os_version().unwrap_or("".to_string()),
                System::kernel_version().unwrap_or("".to_string()),
            )
            .trim()
            .to_owned(),
        ),
        arch: Cow::Owned(System::cpu_arch()),
        core,
        device,
        build_info: Cow::Borrowed(&BUILD_INFO),
    })
}
