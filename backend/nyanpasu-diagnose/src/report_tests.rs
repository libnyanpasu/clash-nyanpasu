use super::report::*;
use crate::host::*;
fn fixture() -> Report {
    let mut report = Report::default();
    report.host(&HostInfo {
        os: "Test OS 1.0".into(),
        arch: "x86_64".into(),
        cpus: vec![CpuInfo {
            brand: "Test CPU".into(),
            frequency_mhz: 3200,
            count: 8,
        }],
        cpu_signature: Some(CpuSignature {
            family: 6,
            model: 183,
            stepping: 1,
        }),
        microcode: Some(0x12f),
        memory_bytes: 16 * 1024_u64.pow(3),
        gpus: vec![GpuInfo {
            name: "Test GPU".into(),
            vendor: "Test vendor".into(),
            kind: GpuKind::Discrete,
            driver_version: "1.2.3".into(),
            vram_mib: 8192,
        }],
        gpu_error: None,
    });
    report.core(CoreInfo {
        name: "mihomo".into(),
        version: Some("Mihomo test version".into()),
        error: None,
    });
    report.webview(WebviewInfo {
        engine: "WebView2".into(),
        version: Some("100.1.2.3".into()),
        browser_args: Some("--test-arg".into()),
        env: [("WEBVIEW2_RELEASE_CHANNEL_PREFERENCE".into(), "1".into())].into(),
        tauri_version: "test-tauri".into(),
        wry_version: "test-wry".into(),
        ..Default::default()
    });
    report.page(serde_json::from_value(serde_json::json!({"userAgent":"Test browser","fullVersionList":[{"brand":"Test Chromium","version":"100.1.2.3"}],"platformVersion":"10.0","webgl":{"vendor":"GL vendor","renderer":"GL renderer"},"webgpu":{"available":true,"adapter":{"vendor":"GPU vendor","architecture":"GPU architecture","device":"GPU device","description":"GPU description"}},"devicePixelRatio":1.5})).unwrap());
    report.build(
        [
            ("app_name", "test-app"),
            ("app_version", "0.1"),
            ("pkg_version", "1.0"),
            ("commit_hash", "test-hash"),
            ("commit_author", "test-author"),
            ("commit_date", "2026-01-01"),
            ("build_date", "2026-01-02"),
            ("build_profile", "Debug"),
            ("build_platform", "test-platform"),
            ("rustc_version", "test-rustc"),
            ("llvm_version", "test-llvm"),
        ]
        .map(|(k, v)| (k.into(), v.into())),
    );
    report
}
#[test]
fn formats_all_sections_in_order() {
    assert_eq!(
        fixture().text(),
        r#"----------- System -----------
OS: Test OS 1.0
Arch: x86_64
----------- Device -----------
CPU: Test CPU @ 3.20GHz x 8
CPU signature: family 6, model 183, stepping 1
Microcode: 0x12f
Memory: 16 GiB
GPU: Test GPU (Test vendor, Discrete, 8192 MiB, driver 1.2.3)
----------- Core -----------
mihomo: `Mihomo test version`
----------- WebView -----------
Engine: WebView2
Version: 100.1.2.3
Browser args: --test-arg
WEBVIEW2_RELEASE_CHANNEL_PREFERENCE: 1
Tauri: test-tauri
Wry: test-wry
----------- Page -----------
User agent: Test browser
Full versions: Test Chromium 100.1.2.3
Platform version: 10.0
WebGL vendor: GL vendor
WebGL renderer: GL renderer
WebGPU: available
WebGPU vendor: GPU vendor
WebGPU architecture: GPU architecture
WebGPU device: GPU device
WebGPU description: GPU description
Device pixel ratio: 1.5
----------- Build Info -----------
App Name: test-app
App Version: 0.1
Pkg Version: 1.0
Commit Hash: test-hash
Commit Author: test-author
Commit Date: 2026-01-01
Build Date: 2026-01-02
Build Profile: Debug
Build Platform: test-platform
Rustc Version: test-rustc
Llvm Version: test-llvm
"#
    );
}
#[test]
fn gpu_failure_keeps_other_facts() {
    let mut report = fixture();
    report.sections[1].lines.retain(|(key, _)| key != "GPU");
    report.sections[1]
        .lines
        .push(("GPU error".into(), "DXGI unavailable".into()));
    let text = report.text();
    assert!(text.contains("GPU error: DXGI unavailable\n"));
    assert!(text.contains("Memory: 16 GiB\n"));
    assert!(text.contains("mihomo: `Mihomo test version`\n"));
}
#[test]
fn missing_core_keeps_host() {
    let mut report = fixture();
    report.sections.retain(|s| s.title != "Core");
    report.core(CoreInfo {
        name: "clash-rs".into(),
        version: None,
        error: Some("binary missing".into()),
    });
    assert!(report.text().contains("clash-rs: Error: binary missing\n"));
    assert!(report.text().contains("GPU: Test GPU"));
}
#[test]
fn partial_pages_never_print_null() {
    for page in [
        serde_json::json!({}),
        serde_json::json!({"webgpu":{"available":false}}),
        serde_json::json!({"webgpu":{"available":true,"adapter":{"vendor":"Vendor"}}}),
    ] {
        let mut report = Report::default();
        report.page(serde_json::from_value(page).unwrap());
        report.build([]);
        assert!(!report.text().contains("null"));
    }
}
#[test]
fn empty_sections_are_left_out() {
    let mut report = Report::default();
    report.page(serde_json::from_value(serde_json::json!({})).unwrap());
    report.build([("App Name".into(), "clash-nyanpasu".into())]);
    let text = report.text();
    assert!(!text.contains("----------- Page -----------"));
    assert!(text.contains("----------- Build Info -----------\nApp Name: clash-nyanpasu\n"));
}
#[test]
fn missing_webview_runtime_omits_overrides() {
    let mut report = Report::default();
    report.webview(WebviewInfo {
        engine: "WebView2".into(),
        version_error: Some("runtime missing".into()),
        ..Default::default()
    });
    let text = report.text();
    assert!(text.contains("Version: Error: runtime missing\n"));
    assert!(!text.contains("Browser args:"));
    assert!(!text.contains("WEBVIEW2_"));
}
