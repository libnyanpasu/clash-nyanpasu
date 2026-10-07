use std::{borrow::Cow, collections::BTreeMap};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use humansize::{BINARY, SizeFormatter};
use nyanpasu_core::diagnostics::{BuildInfo, DeviceInfo, EnvInfo, EnvironmentCollector};
use nyanpasu_paths::PathResolver;
use nyanpasu_utils::core::{ClashCoreType, CoreType};
use sysinfo::System;

pub struct OsEnvironmentCollector {
    build_info: BuildInfo,
    paths: PathResolver,
}

impl OsEnvironmentCollector {
    pub fn new(build_info: BuildInfo, paths: PathResolver) -> Self {
        Self { build_info, paths }
    }
}

impl EnvironmentCollector for OsEnvironmentCollector {
    fn collect(&self) -> Result<EnvInfo<'static>, std::io::Error> {
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

        let mut core = BTreeMap::new();
        for c in CoreType::get_supported_cores() {
            let name: &str = c.as_ref();

            let mut command = std::process::Command::new(
                self.paths
                    .data_or_sidecar_path(name)
                    .map_err(|e| std::io::Error::other(e.to_string()))?,
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
            build_info: Cow::Owned(self.build_info.clone()),
        })
    }
}
