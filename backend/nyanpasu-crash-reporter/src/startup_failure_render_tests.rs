//! Shares the reporter's headless platform and UI thread.
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

use slint::{ComponentHandle as _, PhysicalSize, platform::WindowEvent};

use crate::{StartupFailureInput, StartupFailureText, startup_failure, ui};

fn text(locale: &str, kind: &str) -> StartupFailureText {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../tauri/locales/{locale}.json"));
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let strings = &json["dialog"]["startup_failure"];
    let get = |key: &str| strings[key].as_str().unwrap().to_owned();
    StartupFailureText {
        heading: get("heading"),
        description: strings[kind]["description"].as_str().unwrap().into(),
        recovery: strings[kind]["recovery"].as_str().unwrap().into(),
        path: get("path"),
        details: get("details"),
        open_containing: get("open_containing"),
        open_backups: get("open_backups"),
        open_logs: get("open_logs"),
        copy: get("copy"),
        copied: get("copied"),
        copy_failed: get("copy_failed"),
        open_failed: get("open_failed"),
        exit: get("exit"),
    }
}

pub(crate) fn render_and_exercise_actions(clock: &Cell<Duration>) {
    let preview = std::env::var_os("NYANPASU_STARTUP_PREVIEW_DIR").map(PathBuf::from);
    if let Some(path) = &preview {
        std::fs::create_dir_all(path).unwrap();
    }
    for locale in ["en", "zh-cn", "zh-tw", "ja", "ko", "ru"] {
        for kind in [
            "migration_failed",
            "user_config",
            "core_runtime_directory",
            "internal",
        ] {
            for dark in [false, true] {
                let labels = text(locale, kind);
                let window = ui::StartupFailureWindow::new().unwrap();
                let has_containing = kind != "internal";
                let has_backups = matches!(kind, "migration_failed" | "user_config");
                let path = has_containing.then(|| {
                    PathBuf::from(if kind == "core_runtime_directory" {
                        "C:/用户/配置/Clash Nyanpasu/runtime/control"
                    } else {
                        "C:/用户/配置/Clash Nyanpasu/application.yaml"
                    })
                });
                let details = format!(
                    "Clash Nyanpasu 2.0.0 (7033e1f44)\n{}\nC:/用户/日志/Clash Nyanpasu\n\n{}\n\nCaused by:\n  1: failed to read application configuration\n  2: invalid value at line 18 column 7",
                    path.as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    labels.description
                );
                let input = StartupFailureInput {
                    version: "Clash Nyanpasu 2.0.0 (7033e1f44)".into(),
                    path,
                    details: details.clone(),
                    containing_dir: has_containing.then(|| "config".into()),
                    backups_dir: has_backups.then(|| "backups".into()),
                    logs_dir: Some("logs".into()),
                };
                startup_failure::fill_window(&window, &input, &labels);
                assert_eq!(window.get_heading(), labels.heading);
                assert_eq!(window.get_description(), labels.description);
                assert_eq!(window.get_recovery(), labels.recovery);
                assert_eq!(window.get_details(), details);
                assert_eq!(window.get_containing_label(), labels.open_containing);
                assert_eq!(window.get_backups_label(), labels.open_backups);
                assert_eq!(window.get_logs_label(), labels.open_logs);
                assert_eq!(window.get_copy_label(), labels.copy);
                assert_eq!(window.get_exit_label(), labels.exit);
                assert_eq!(window.get_has_containing(), has_containing);
                assert_eq!(window.get_has_backups(), has_backups);
                assert!(window.get_has_logs());
                let opened = Rc::new(RefCell::new(Vec::new()));
                let copied = Rc::new(RefCell::new(String::new()));
                let exits = Rc::new(Cell::new(0));
                startup_failure::bind_actions(
                    &window,
                    input,
                    &labels,
                    {
                        let copied = copied.clone();
                        move |value| {
                            copied.replace(value.into());
                            true
                        }
                    },
                    {
                        let opened = opened.clone();
                        move |path| {
                            opened.borrow_mut().push(path.to_owned());
                            true
                        }
                    },
                    {
                        let exits = exits.clone();
                        move || exits.set(exits.get() + 1)
                    },
                );
                window.global::<ui::Palette>().set_color_scheme(if dark {
                    slint::language::ColorScheme::Dark
                } else {
                    slint::language::ColorScheme::Light
                });
                for (width, height, scale) in [(760, 620, 1.0), (760, 620, 2.0), (660, 540, 1.0)] {
                    window
                        .window()
                        .dispatch_event(WindowEvent::ScaleFactorChanged {
                            scale_factor: scale,
                        });
                    window.window().set_size(PhysicalSize::new(
                        (width as f32 * scale) as u32,
                        (height as f32 * scale) as u32,
                    ));
                    window.window().take_snapshot().unwrap();
                    clock.set(clock.get() + Duration::from_secs(1));
                    slint::platform::update_timers_and_animations();
                    let pixels = window.window().take_snapshot().unwrap();
                    assert!(
                        pixels.as_slice().windows(2).any(|p| p[0] != p[1]),
                        "window must paint content"
                    );
                    if let Some(path) = &preview
                        && scale == 1.0
                        && ((matches!(locale, "en" | "zh-cn") && width == 760)
                            || (locale == "ru" && kind == "migration_failed" && width == 660))
                    {
                        let path = path.join(format!(
                            "{locale}-{}-{kind}{}.png",
                            if dark { "dark" } else { "light" },
                            if width == 660 { "-min" } else { "" }
                        ));
                        let file = std::fs::File::create(path).unwrap();
                        let mut encoder = png::Encoder::new(file, pixels.width(), pixels.height());
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        encoder
                            .write_header()
                            .unwrap()
                            .write_image_data(pixels.as_bytes())
                            .unwrap();
                    }
                    if locale == "ru" && kind == "migration_failed" && width == 660 && scale == 1.0
                    {
                        assert!(
                            window.get_has_containing()
                                && window.get_has_backups()
                                && window.get_has_logs()
                        );
                        assert_eq!((pixels.width(), pixels.height()), (660, 540));
                        let background = pixels.as_slice()[0];
                        // The content has 24px padding. Ignore one boundary pixel for raster rounding.
                        assert!(
                            pixels
                                .as_slice()
                                .chunks(pixels.width() as usize)
                                .all(|row| {
                                    row[row.len() - 23..]
                                        .iter()
                                        .all(|pixel| *pixel == background)
                                }),
                            "Russian folder actions must fit inside the window padding"
                        );
                    }
                    // Exercise the actual Slint buttons as well as the Rust callbacks.
                    // These points are inside the reviewed 760x620 English layout.
                    if locale == "en" && kind == "migration_failed" && width == 760 && scale == 1.0
                    {
                        let click = |x, y| {
                            let position = slint::LogicalPosition::new(x, y);
                            window.window().dispatch_event(WindowEvent::PointerPressed {
                                position,
                                button: slint::platform::PointerEventButton::Left,
                            });
                            window
                                .window()
                                .dispatch_event(WindowEvent::PointerReleased {
                                    position,
                                    button: slint::platform::PointerEventButton::Left,
                                });
                        };
                        for x in [100.0, 270.0, 420.0] {
                            click(x, 213.0);
                        }
                        assert_eq!(
                            *opened.borrow(),
                            [
                                PathBuf::from("config"),
                                PathBuf::from("backups"),
                                PathBuf::from("logs")
                            ]
                        );
                        click(70.0, 580.0);
                        assert_eq!(*copied.borrow(), details);
                        assert_eq!(exits.get(), 0);
                        click(710.0, 580.0);
                        assert_eq!(exits.get(), 1);
                        opened.borrow_mut().clear();
                        copied.borrow_mut().clear();
                        exits.set(0);
                        window.set_status("".into());
                    }
                }
                window.invoke_open_containing();
                window.invoke_open_backups();
                window.invoke_open_logs();
                window.invoke_copy_details();
                assert_eq!(*copied.borrow(), details);
                assert_eq!(window.get_status(), labels.copied);
                let expected: Vec<PathBuf> = [
                    has_containing.then_some("config"),
                    has_backups.then_some("backups"),
                    Some("logs"),
                ]
                .into_iter()
                .flatten()
                .map(Into::into)
                .collect();
                assert_eq!(*opened.borrow(), expected);
                assert_eq!(
                    exits.get(),
                    0,
                    "folder/copy actions must keep the window open"
                );
                window.invoke_exit();
                assert_eq!(exits.get(), 1);
            }
        }
    }
    let window = ui::StartupFailureWindow::new().unwrap();
    let labels = text("en", "internal");
    let input = StartupFailureInput {
        version: "test".into(),
        path: None,
        details: "test".into(),
        containing_dir: None,
        backups_dir: None,
        logs_dir: Some("logs".into()),
    };
    startup_failure::fill_window(&window, &input, &labels);
    let copy_ok = Rc::new(Cell::new(false));
    let open_ok = Rc::new(Cell::new(false));
    let exits = Rc::new(Cell::new(0));
    startup_failure::bind_actions(
        &window,
        input,
        &labels,
        {
            let copy_ok = copy_ok.clone();
            move |_| copy_ok.get()
        },
        {
            let open_ok = open_ok.clone();
            move |_| open_ok.get()
        },
        {
            let exits = exits.clone();
            move || exits.set(exits.get() + 1)
        },
    );
    window.window().set_size(PhysicalSize::new(760, 620));
    for dark in [false, true] {
        window.global::<ui::Palette>().set_color_scheme(if dark {
            slint::language::ColorScheme::Dark
        } else {
            slint::language::ColorScheme::Light
        });
        for copy in [false, true] {
            if copy {
                window.invoke_copy_details();
                assert_eq!(window.get_status(), labels.copy_failed);
            } else {
                window.invoke_open_logs();
                assert_eq!(window.get_status(), labels.open_failed);
            }
            window.window().take_snapshot().unwrap();
            clock.set(clock.get() + Duration::from_secs(1));
            slint::platform::update_timers_and_animations();
            let pixels = window.window().take_snapshot().unwrap();
            let warning = if dark { (255, 204, 102) } else { (138, 87, 0) };
            assert!(
                pixels
                    .as_slice()
                    .iter()
                    .any(|pixel| (pixel.r, pixel.g, pixel.b) == warning),
                "failed copy/open status must use the reporter warning tint (dark={dark}, copy={copy})"
            );
        }
        open_ok.set(true);
        window.invoke_open_logs();
        assert!(
            window.get_status().is_empty(),
            "a successful folder open clears the copy failure"
        );
        assert!(
            !window.get_status_is_error(),
            "a successful folder open resets the warning tint"
        );
        open_ok.set(false);
    }
    copy_ok.set(true);
    window.show().unwrap();
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Return.into(),
    });
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Return.into(),
    });
    assert_eq!(
        window.get_status(),
        labels.copied,
        "initial Return copies details"
    );
    assert_eq!(exits.get(), 0, "initial focus must not exit");
    assert!(
        !window.get_status_is_error(),
        "successful copying clears the failure tint"
    );
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Escape.into(),
    });
    assert_eq!(exits.get(), 1, "Escape still exits");
    window.hide().unwrap();
}
