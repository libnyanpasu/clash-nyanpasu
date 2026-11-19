//! Renders the window with the software renderer, without an OS window.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use nyanpasu_slint::testing::HeadlessPlatform;
use slint::{
    ComponentHandle as _, LogicalPosition, PhysicalSize, Rgba8Pixel, SharedPixelBuffer,
    platform::{Key, PointerEventButton, WindowEvent},
};

use crate::{CrashReport, PanicDetails, ReporterInput, ReporterText, fill_window, ui};

slint::slint! {
    #[edition = "1.19"]
    export component GlyphProbe inherits Window {
        width: 48px;
        height: 48px;
        background: white;
        in property <string> glyph;
        Text {
            text: root.glyph;
            font-family: "Nyanpasu Test Sparse";
            font-size: 24px;
            color: black;
        }
    }
}

fn cjk_glyphs_survive_a_sparse_system_fallback() {
    use slint::fontique_012::fontique::{Blob, FallbackKey, GenericFamily, Script};

    let mut collection = slint::fontique_012::shared_collection();
    let sparse = collection.register_fonts(
        Blob::new(std::sync::Arc::new(
            include_bytes!("../tests/fonts/sparse.ttf").as_slice(),
        )),
        None,
    )[0]
    .0;
    collection.register_fonts(
        Blob::new(std::sync::Arc::new(
            include_bytes!("../tests/fonts/cjk.ttf").as_slice(),
        )),
        None,
    );
    collection.set_fallbacks(
        FallbackKey::new(Script::from_bytes(*b"Hani"), None),
        std::iter::once(sparse),
    );
    // Neither the explicit primary nor Slint's generic families may borrow
    // glyphs from installed fonts and hide an incomplete script fallback.
    for generic in [GenericFamily::SansSerif, GenericFamily::SystemUi] {
        collection.set_generic_families(generic, std::iter::once(sparse));
    }
    crate::platform::extend_cjk_fallbacks(
        &mut collection,
        &["Nyanpasu Missing Font", "Nyanpasu Test CJK"],
    );

    // These characters disappeared with the Japanese system fallback. Check
    // actual ink, not only the existence or dimensions of the snapshot.
    for ch in "报告问题系统环境请在描述帮助我们修复诊断信息已就绪文件夹中显示复制关闭".chars()
    {
        let probe = GlyphProbe::new().unwrap();
        probe.window().set_size(PhysicalSize::new(48, 48));
        probe.set_glyph(ch.to_string().into());
        let pixels = probe.window().take_snapshot().unwrap();
        assert!(
            pixels
                .as_slice()
                .iter()
                .any(|p| p.r < 128 && p.g < 128 && p.b < 128),
            "missing rendered glyph for {ch}"
        );
    }
}

fn sample() -> CrashReport {
    CrashReport {
        app: "2.0.0-alpha+autobuild.1023 (e8fe65e31)".into(),
        os: "Windows 11 Pro 26300 (x86_64)".into(),
        panic: PanicDetails {
            parent_pid: 123,
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
        manual_heading: get("manual_heading"),
        manual_description: get("manual_description"),
        collecting: get("collecting"),
        steps: [
            get("step_host"),
            get("step_webview"),
            get("step_core"),
            get("step_app_logs"),
            get("step_core_logs"),
        ],
        ready: get("ready"),
        failed: get("failed"),
        interrupted: get("interrupted"),
        show_in_folder: get("show_in_folder"),
        heading: get("heading"),
        description: get("description"),
        version: get("version"),
        system: get("system"),
        error: get("error"),
        location: get("location"),
        backtrace: get("backtrace"),
        environment: get("environment"),
        copy: get("copy"),
        copied: get("copied"),
        copy_section: get("copy_section"),
        section_copied: get("section_copied"),
        copy_failed: get("copy_failed"),
        close: get("close"),
        report_issue: get("report_issue"),
        issue_opened: get("issue_opened"),
        open_failed: get("open_failed"),
        issue_template: "bug_report.yaml".into(),
    }
}

/// Snapshots the window once its transitions have finished.
fn settled(window: &slint::Window, clock: &Cell<Duration>) -> SharedPixelBuffer<Rgba8Pixel> {
    window.take_snapshot().unwrap();
    clock.set(clock.get() + Duration::from_secs(1));
    slint::platform::update_timers_and_animations();
    window.take_snapshot().unwrap()
}

fn ctrl(window: &slint::Window, key: char) {
    let control = slint::SharedString::from(Key::Control);
    window.dispatch_event(WindowEvent::KeyPressed {
        text: control.clone(),
    });
    window.dispatch_event(WindowEvent::KeyPressed { text: key.into() });
    window.dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    window.dispatch_event(WindowEvent::KeyReleased { text: control });
}

/// The report pane: hovering reveals its copy button, each pane opens at its
/// top, the text selects and copies exactly, and Escape still closes.
fn report_pane_scrolls_selects_and_copies(
    clock: &Cell<Duration>,
    clipboard: &RefCell<Option<String>>,
) {
    let window = ui::ReporterWindow::new().unwrap();
    fill_window(&window, &ReporterInput::Crash(sample()), &text("en"));
    // Long enough to scroll, as a real environment is.
    window.set_environment_text(
        (0..100)
            .map(|line| format!("{line}\n"))
            .collect::<String>()
            .into(),
    );
    let closed = Rc::new(Cell::new(false));
    window.on_close_report({
        let closed = closed.clone();
        move || closed.set(true)
    });
    window.show().unwrap();
    window
        .window()
        .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1. });
    window.window().set_size(PhysicalSize::new(720, 620));
    let away = settled(window.window(), clock);

    // Inside the report, which fills the window below the pane switch.
    let inside = LogicalPosition::new(300., 420.);
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position: inside });
    let top = settled(window.window(), clock);
    assert!(
        away.as_bytes() != top.as_bytes(),
        "hovering the report reveals its copy button"
    );

    window
        .window()
        .dispatch_event(WindowEvent::PointerScrolled {
            position: inside,
            delta_x: 0.,
            delta_y: -3. * 60.,
        });
    assert!(
        top.as_bytes() != settled(window.window(), clock).as_bytes(),
        "the wheel scrolls the backtrace"
    );
    window.set_pane(1);
    settled(window.window(), clock);
    window.set_pane(0);
    assert!(
        top.as_bytes() == settled(window.window(), clock).as_bytes(),
        "the backtrace reopens at its top"
    );

    let button = PointerEventButton::Left;
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: inside,
        button,
    });
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: inside,
            button,
        });
    ctrl(window.window(), 'a');
    ctrl(window.window(), 'c');
    assert_eq!(
        clipboard.borrow().as_deref(),
        Some(sample().text().as_str())
    );

    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Escape.into(),
    });
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Escape.into(),
    });
    assert!(closed.get(), "Escape closes from inside the report");
    window.hide().unwrap();
}

/// One test: Slint's platform is per process and components stay on their thread.
#[test]
fn window_renders_in_both_schemes_and_scales() {
    let platform = HeadlessPlatform::default();
    let clock = platform.clock();
    let clipboard = platform.clipboard();
    slint::platform::set_platform(Box::new(platform)).unwrap();
    crate::platform::configure_fonts();
    let window = ui::ReporterWindow::new().unwrap();
    for locale in ["en", "zh-cn", "zh-tw", "ja", "ko", "ru"] {
        fill_window(&window, &ReporterInput::Crash(sample()), &text(locale));
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

    // Crash/manual progress, finished, and partial-failure states. Preview output
    // is opt-in so ordinary tests leave the checkout and user directories alone.
    let preview = std::env::var_os("NYANPASU_DIAGNOSE_PREVIEW_DIR").map(std::path::PathBuf::from);
    if let Some(path) = &preview {
        std::fs::create_dir_all(path).unwrap();
    }
    let order = [
        crate::Step::Host,
        crate::Step::Webview,
        crate::Step::Core,
        crate::Step::AppLogs,
        crate::Step::CoreLogs,
    ];
    for locale in ["en", "zh-cn", "zh-tw", "ja", "ko", "ru"] {
        for dark in [false, true] {
            for manual in [false, true] {
                let input = if manual {
                    ReporterInput::Manual {
                        app: sample().app,
                        os: sample().os,
                    }
                } else {
                    ReporterInput::Crash(sample())
                };
                for state in ["progress", "finished", "failed"] {
                    let window = ui::ReporterWindow::new().unwrap();
                    let labels = text(locale);
                    fill_window(&window, &input, &labels);
                    window.global::<ui::Palette>().set_color_scheme(if dark {
                        slint::language::ColorScheme::Dark
                    } else {
                        slint::language::ColorScheme::Light
                    });
                    window
                        .window()
                        .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1. });
                    window.window().set_size(PhysicalSize::new(720, 620));
                    window.window().take_snapshot().unwrap();
                    let event = if state == "progress" {
                        crate::Event::Progress {
                            step: crate::Step::Core,
                            fraction: 0.,
                        }
                    } else {
                        let environment = "----------- System -----------\nOS: Windows 11 Pro 26300\nArch: x86_64\n----------- Device -----------\nCPU: Test CPU @ 3.20GHz x 8\nMemory: 16 GiB\n";
                        crate::Event::Finished {
                            report: format!("{}{environment}{}", input.head(), input.tail()),
                            environment: environment.into(),
                            archive: Some("diagnostics.zip".into()),
                            problems: if state == "failed" {
                                vec![nyanpasu_diagnose::Problem {
                                    step: crate::Step::CoreLogs,
                                    message: "Store locked".into(),
                                }]
                            } else {
                                vec![]
                            },
                        }
                    };
                    crate::apply_event(&window, event, &order, &crate::EventText::from(&labels));
                    if state == "progress" {
                        assert!((window.get_progress() - 0.4).abs() < 0.001);
                        assert!(!window.get_finished());
                    } else {
                        assert!(window.get_finished());
                        assert!(window.get_environment_text().starts_with("-----------"));
                        // The finished crash previews show the environment pane.
                        if !manual {
                            window.set_pane(1);
                        }
                        assert_eq!(window.get_has_problems(), state == "failed");
                        if state == "finished" {
                            window.set_progress_label(format!("{} · 1.2 MiB", labels.ready).into());
                        }
                    }
                    // Evaluate changed states, then advance beyond Fluent's 150ms
                    // transitions. Preview pixels show the final enabled/disabled state.
                    window.window().take_snapshot().unwrap();
                    clock.set(clock.get() + Duration::from_secs(1));
                    slint::platform::update_timers_and_animations();
                    let pixels = window.window().take_snapshot().unwrap();
                    assert_eq!(pixels.width(), 720);
                    assert_eq!(pixels.height(), 620);
                    if let Some(path) = &preview {
                        let name = format!(
                            "{locale}-{}-{}-{state}.png",
                            if dark { "dark" } else { "light" },
                            if manual { "manual" } else { "crash" }
                        );
                        let file = std::fs::File::create(path.join(name)).unwrap();
                        let mut encoder = png::Encoder::new(file, pixels.width(), pixels.height());
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        encoder
                            .write_header()
                            .unwrap()
                            .write_image_data(pixels.as_bytes())
                            .unwrap();
                    }
                }
            }
        }
    }
    // A crash opens on its backtrace; the whole report is what gets copied.
    fill_window(&window, &ReporterInput::Crash(sample()), &text("en"));
    assert_eq!(window.get_pane(), 0);
    assert_eq!(window.get_backtrace_text(), sample().text());
    assert_eq!(window.get_report_text(), sample().text());

    // Every locale lays out at the smallest supported size, with a three-line error.
    let mut report = sample();
    report.panic.payload = Some("index out of bounds: ".repeat(40));
    for locale in ["en", "zh-cn", "zh-tw", "ja", "ko", "ru"] {
        fill_window(
            &window,
            &ReporterInput::Crash(report.clone()),
            &text(locale),
        );
        window
            .window()
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
        window.window().set_size(PhysicalSize::new(520, 480));
        window.window().take_snapshot().unwrap();
    }
    window.set_monospace_font("Nyanpasu Missing Test Font".into());
    window.window().take_snapshot().unwrap();
    report_pane_scrolls_selects_and_copies(&clock, &clipboard);
    crate::startup_failure_render_tests::render_and_exercise_actions(&clock);
    cjk_glyphs_survive_a_sparse_system_fallback();
}
