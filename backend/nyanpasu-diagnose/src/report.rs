//! Ordered environment sections and the page capability wire model.
use crate::host::HostInfo;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct PageCapabilities {
    #[specta(optional)]
    pub user_agent: Option<String>,
    #[specta(optional)]
    pub full_version_list: Option<Vec<BrandVersion>>,
    #[specta(optional)]
    pub platform_version: Option<String>,
    #[specta(optional)]
    pub webgl: Option<WebGl>,
    #[specta(optional)]
    pub webgpu: Option<WebGpu>,
    #[specta(optional)]
    pub device_pixel_ratio: Option<f64>,
}
#[derive(Debug, Deserialize, specta::Type)]
pub struct BrandVersion {
    pub brand: String,
    pub version: String,
}
#[derive(Debug, Default, Deserialize, specta::Type)]
#[serde(default)]
pub struct WebGl {
    #[specta(optional)]
    pub vendor: Option<String>,
    #[specta(optional)]
    pub renderer: Option<String>,
}
#[derive(Debug, Deserialize, specta::Type)]
pub struct WebGpu {
    pub available: bool,
    #[specta(optional)]
    pub adapter: Option<WebGpuAdapter>,
}
#[derive(Debug, Default, Deserialize, specta::Type)]
#[serde(default)]
pub struct WebGpuAdapter {
    #[specta(optional)]
    pub vendor: Option<String>,
    #[specta(optional)]
    pub architecture: Option<String>,
    #[specta(optional)]
    pub device: Option<String>,
    #[specta(optional)]
    pub description: Option<String>,
}
#[derive(Debug, Default)]
pub struct WebviewInfo {
    pub engine: String,
    pub version: Option<String>,
    pub version_error: Option<String>,
    pub browser_args: Option<String>,
    pub env: BTreeMap<String, String>,
    pub tauri_version: String,
    pub wry_version: String,
}
#[derive(Debug)]
pub struct CoreInfo {
    pub name: String,
    pub version: Option<String>,
    pub error: Option<String>,
}
#[derive(Debug, Default)]
pub struct Section {
    pub title: String,
    pub lines: Vec<(String, String)>,
}
#[derive(Debug, Default)]
pub struct Report {
    /// Text before the environment, such as a crash's error and location.
    pub prefix: String,
    /// Text after the environment, such as a crash's backtrace.
    pub suffix: String,
    pub sections: Vec<Section>,
    pub notes: Vec<String>,
}
impl Report {
    pub fn section(&mut self, title: &str, lines: impl IntoIterator<Item = (String, String)>) {
        self.sections.push(Section {
            title: title.into(),
            lines: lines.into_iter().collect(),
        });
    }
    pub fn host(&mut self, host: &HostInfo) {
        self.section(
            "System",
            [
                ("OS".into(), host.os.clone()),
                ("Arch".into(), host.arch.clone()),
            ],
        );
        let mut lines = Vec::new();
        for cpu in &host.cpus {
            lines.push((
                "CPU".into(),
                format!(
                    "{} @ {:.2}GHz x {}",
                    cpu.brand,
                    cpu.frequency_mhz as f64 / 1000.,
                    cpu.count
                ),
            ));
        }
        if let Some(signature) = host.cpu_signature {
            lines.push((
                "CPU signature".into(),
                format!(
                    "family {}, model {}, stepping {}",
                    signature.family, signature.model, signature.stepping
                ),
            ));
        }
        if let Some(microcode) = host.microcode {
            lines.push(("Microcode".into(), format!("{microcode:#x}")));
        }
        lines.push(("Memory".into(), format_bytes(host.memory_bytes)));
        for gpu in &host.gpus {
            lines.push((
                "GPU".into(),
                format!(
                    "{} ({}, {:?}, {} MiB, driver {})",
                    gpu.name,
                    gpu.vendor,
                    gpu.kind,
                    gpu.vram_mib,
                    if gpu.driver_version.is_empty() {
                        "unknown"
                    } else {
                        &gpu.driver_version
                    }
                ),
            ));
        }
        if let Some(error) = &host.gpu_error {
            lines.push(("GPU error".into(), error.clone()));
        }
        self.section("Device", lines);
    }
    pub fn core(&mut self, core: CoreInfo) {
        self.section(
            "Core",
            [(
                core.name,
                match core.error {
                    Some(error) => format!("Error: {error}"),
                    None => format!("`{}`", core.version.as_deref().unwrap_or("unknown")),
                },
            )],
        );
    }
    pub fn webview(&mut self, webview: WebviewInfo) {
        let mut lines = vec![
            ("Engine".into(), webview.engine),
            (
                "Version".into(),
                webview
                    .version_error
                    .map(|error| format!("Error: {error}"))
                    .unwrap_or_else(|| webview.version.unwrap_or_else(|| "unknown".into())),
            ),
        ];
        if let Some(args) = webview.browser_args {
            lines.push(("Browser args".into(), args));
        }
        lines.extend(webview.env);
        lines.push(("Tauri".into(), webview.tauri_version));
        lines.push(("Wry".into(), webview.wry_version));
        self.section("WebView", lines);
    }
    pub fn page(&mut self, page: PageCapabilities) {
        let mut lines = Vec::new();
        let mut push = |key: &str, value: Option<String>| {
            if let Some(value) = value {
                lines.push((key.into(), value));
            }
        };
        push("User agent", page.user_agent);
        push(
            "Full versions",
            page.full_version_list.map(|versions| {
                versions
                    .into_iter()
                    .map(|v| format!("{} {}", v.brand, v.version))
                    .collect::<Vec<_>>()
                    .join(", ")
            }),
        );
        push("Platform version", page.platform_version);
        if let Some(gl) = page.webgl {
            push("WebGL vendor", gl.vendor);
            push("WebGL renderer", gl.renderer);
        }
        if let Some(gpu) = page.webgpu {
            push(
                "WebGPU",
                Some(
                    if gpu.available {
                        "available"
                    } else {
                        "unavailable"
                    }
                    .into(),
                ),
            );
            if let Some(adapter) = gpu.adapter {
                push("WebGPU vendor", adapter.vendor);
                push("WebGPU architecture", adapter.architecture);
                push("WebGPU device", adapter.device);
                push("WebGPU description", adapter.description);
            }
        }
        push(
            "Device pixel ratio",
            page.device_pixel_ratio.map(|ratio| ratio.to_string()),
        );
        self.section("Page", lines);
    }
    pub fn build(&mut self, pairs: impl IntoIterator<Item = (String, String)>) {
        self.section(
            "Build Info",
            pairs.into_iter().map(|(key, value)| {
                (
                    key.split('_')
                        .map(|word| {
                            let mut chars = word.chars();
                            chars
                                .next()
                                .map(|c| c.to_uppercase().to_string() + chars.as_str())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                    value,
                )
            }),
        );
    }
    /// The prefix, the environment and the suffix: the archive's report.
    pub fn text(&self) -> String {
        let mut text = self.prefix.clone();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&self.environment());
        text.push_str(&self.suffix);
        text
    }

    /// The sections and the notes, without the prefix and the suffix.
    pub fn environment(&self) -> String {
        let mut text = String::new();
        // Collection order is independent of the presentation order.
        // A section with nothing to say, such as Page outside the GUI, is left out.
        let mut sections: Vec<_> = self
            .sections
            .iter()
            .filter(|section| !section.lines.is_empty())
            .collect();
        sections.sort_by_key(|section| match section.title.as_str() {
            "System" => 0,
            "Device" => 1,
            "Core" => 2,
            "WebView" => 3,
            "Page" => 4,
            "Build Info" => 5,
            "Diagnostics" => 7,
            _ => 6,
        });
        for section in sections {
            text.push_str(&format!("----------- {} -----------\n", section.title));
            for (key, value) in &section.lines {
                text.push_str(&format!("{key}: {value}\n"));
            }
        }
        if !self.notes.is_empty() {
            text.push_str("----------- Diagnostics -----------\n");
            for note in &self.notes {
                text.push_str(note);
                text.push('\n');
            }
        }
        text
    }
}
pub fn format_bytes(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"] {
        if value < 1024. {
            break;
        }
        value /= 1024.;
        unit = next;
    }
    let rounded = format!("{value:.2}");
    format!(
        "{} {unit}",
        rounded.trim_end_matches('0').trim_end_matches('.')
    )
}
