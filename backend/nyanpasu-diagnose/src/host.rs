//! OS facts collected without enumerating processes, disks or networks.

mod cpu;

pub use cpu::CpuSignature;
use serde::Serialize;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

#[derive(Debug, Serialize)]
pub struct HostInfo {
    pub os: String,
    pub arch: String,
    pub cpus: Vec<CpuInfo>,
    pub cpu_signature: Option<CpuSignature>,
    /// The loaded microcode revision, when the platform exposes it.
    pub microcode: Option<u32>,
    pub memory_bytes: u64,
    pub gpus: Vec<GpuInfo>,
    pub gpu_error: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CpuInfo {
    pub brand: String,
    pub frequency_mhz: u64,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub kind: GpuKind,
    pub driver_version: String,
    pub vram_mib: u64,
}

#[derive(Debug, Serialize)]
pub enum GpuKind {
    Integrated,
    Discrete,
    Virtual,
    Cpu,
    Unknown,
}

impl From<gpu_info::GPU> for GpuInfo {
    fn from(gpu: gpu_info::GPU) -> Self {
        Self {
            name: gpu.name,
            vendor: gpu.vendor,
            kind: match gpu.kind {
                gpu_info::GPUKind::Integrated => GpuKind::Integrated,
                gpu_info::GPUKind::Discrete => GpuKind::Discrete,
                gpu_info::GPUKind::Virtual => GpuKind::Virtual,
                gpu_info::GPUKind::CPU => GpuKind::Cpu,
                gpu_info::GPUKind::Unknown => GpuKind::Unknown,
            },
            driver_version: gpu.driver_version,
            // The pinned fork returns MiB on all three backends: DXGI divides
            // dedicated bytes, Vulkan divides heap bytes, and Metal divides its
            // working-set bytes or reads IOKit's VRAM,totalMB directly.
            vram_mib: gpu.vram,
        }
    }
}

pub fn collect_host_info() -> HostInfo {
    let system = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_frequency())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    let (gpus, gpu_error) = match gpu_info::retrieve_gpu_info() {
        Ok(gpus) => (gpus.into_iter().map(GpuInfo::from).collect(), None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };

    HostInfo {
        os: format!(
            "{} {}",
            System::long_os_version().unwrap_or_default(),
            System::kernel_version().unwrap_or_default(),
        )
        .trim()
        .to_owned(),
        arch: System::cpu_arch(),
        cpus: group_cpus(
            system
                .cpus()
                .iter()
                .map(|cpu| (cpu.brand(), cpu.frequency())),
        ),
        cpu_signature: cpu::signature(),
        microcode: cpu::microcode(),
        memory_bytes: system.total_memory(),
        gpus,
        gpu_error,
    }
}

fn group_cpus<'a>(cpus: impl IntoIterator<Item = (&'a str, u64)>) -> Vec<CpuInfo> {
    let mut groups: Vec<CpuInfo> = Vec::new();
    for (brand, frequency_mhz) in cpus {
        match groups.iter_mut().find(|cpu| cpu.brand == brand) {
            Some(cpu) => cpu.count += 1,
            None => groups.push(CpuInfo {
                brand: brand.to_owned(),
                frequency_mhz,
                count: 1,
            }),
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_cpus_by_brand_preserving_first_frequency_and_order() {
        assert_eq!(
            group_cpus([("CPU B", 3200), ("CPU A", 2400), ("CPU B", 1800)]),
            vec![
                CpuInfo {
                    brand: "CPU B".to_owned(),
                    frequency_mhz: 3200,
                    count: 2,
                },
                CpuInfo {
                    brand: "CPU A".to_owned(),
                    frequency_mhz: 2400,
                    count: 1,
                },
            ],
        );
    }

    #[test]
    fn groups_no_cpus() {
        assert!(group_cpus([]).is_empty());
    }

    #[test]
    fn collects_host_os_and_cpus() {
        let host = collect_host_info();
        assert!(!host.os.is_empty());
        assert!(!host.cpus.is_empty());
    }
}
