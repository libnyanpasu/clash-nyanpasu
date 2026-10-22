//! Renders the window with the software renderer, without an OS window.

use std::rc::Rc;

use slint::{
    ComponentHandle as _, PhysicalSize, PlatformError,
    platform::{
        Platform, WindowAdapter, WindowEvent,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};

use crate::{CrashReport, PanicDetails, ReporterText, fill_window, ui};

struct HeadlessPlatform;

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

fn sample() -> CrashReport {
    CrashReport {
        app: "2.0.0-alpha+autobuild.1023 (e8fe65e31)".into(),
        os: "Windows 11 Pro 26300 (x86_64)".into(),
        panic: PanicDetails {
            payload: Some("index out of bounds: the len is 269 but the index is 269".into()),
            location: Some("nyanpasu-tsdb\\src\\catalog\\paged.rs:505:9".into()),
            backtrace: (0..60_u32)
                .map(|frame| {
                    let module = match frame {
                        58 => "KERNEL32.DLL",
                        59 => "ntdll.dll",
                        _ => "clash-nyanpasu.exe",
                    };
                    let offset = 0x1a2b3c + frame.wrapping_mul(0x45d9f3b) % 0x1800000;
                    format!("{frame:4}: {module}+0x{offset:x}\n")
                })
                .collect(),
        },
    }
}

fn text(locale: &str) -> ReporterText {
    let path = format!("../tauri/locales/{locale}.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let strings = &json["dialog"]["crash_reporter"];
    let get = |key: &str| strings[key].as_str().unwrap().to_owned();
    ReporterText {
        heading: get("heading"),
        description: get("description"),
        version: get("version"),
        system: get("system"),
        error: get("error"),
        location: get("location"),
        report: get("report"),
        copy: get("copy"),
        copied: get("copied"),
        copy_failed: get("copy_failed"),
        close: get("close"),
        report_issue: get("report_issue"),
        issue_opened: get("issue_opened"),
        open_failed: get("open_failed"),
        issue_template: "bug_report.yaml".into(),
    }
}

/// One test: Slint's platform is per process and components stay on their thread.
#[test]
fn window_renders_in_both_schemes_and_scales() {
    slint::platform::set_platform(Box::new(HeadlessPlatform)).unwrap();
    let window = ui::ReporterWindow::new().unwrap();
    for locale in ["en", "zh-cn"] {
        fill_window(&window, &sample(), &text(locale));
        for dark in [false, true] {
            window.global::<ui::Palette>().set_color_scheme(if dark {
                slint::language::ColorScheme::Dark
            } else {
                slint::language::ColorScheme::Light
            });
            for scale in [1.0, 2.0] {
                window
                    .window()
                    .dispatch_event(WindowEvent::ScaleFactorChanged {
                        scale_factor: scale,
                    });
                window.window().set_size(PhysicalSize::new(
                    (720.0 * scale) as u32,
                    (540.0 * scale) as u32,
                ));
                let pixels = window.window().take_snapshot().unwrap();
                assert_eq!(pixels.width(), (720.0 * scale) as u32);
                assert_eq!(pixels.height(), (540.0 * scale) as u32);
            }
        }
    }

    // Every locale lays out at the smallest supported size, with a three-line error.
    let mut report = sample();
    report.panic.payload = Some("index out of bounds: ".repeat(40));
    for locale in ["en", "zh-cn", "zh-tw", "ja", "ko", "ru"] {
        fill_window(&window, &report, &text(locale));
        window
            .window()
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
        window.window().set_size(PhysicalSize::new(520, 400));
        window.window().take_snapshot().unwrap();
    }
    window.set_monospace_font("Nyanpasu Missing Test Font".into());
    window.window().take_snapshot().unwrap();
}
