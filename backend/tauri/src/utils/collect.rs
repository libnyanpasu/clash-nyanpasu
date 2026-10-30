//! Desktop adapters and composition for the shared diagnosis.
use crate::{
    client::{NyanpasuClient, core_version::CoreVersionReader},
    core::logs::{CoreLogExport, CoreLogStore, RedbCoreLogStore},
};
use nyanpasu_config::application::NyanpasuAppConfig;
use nyanpasu_core::format::Format;
use nyanpasu_diagnose::{
    Action, Event, Step,
    archive::{Archive, LOG_CAP},
    report::{CoreInfo, PageCapabilities, Report},
};
use nyanpasu_paths::PathResolver;
use std::{io::Write, process::Stdio, time::Duration};

fn home() -> Option<String> {
    dirs::home_dir().map(|path| path.to_string_lossy().into_owned())
}
fn initial_report((prefix, suffix): (String, String), page: PageCapabilities) -> Report {
    let mut report = Report {
        prefix,
        suffix,
        ..Default::default()
    };
    report.page(page);
    let build =
        serde_json::to_value(&*crate::consts::BUILD_INFO).expect("build metadata serializes");
    report.build(
        build
            .as_object()
            .expect("build metadata is an object")
            .iter()
            .map(|(key, value)| (key.clone(), value.as_str().unwrap_or_default().to_owned())),
    );
    report
}
fn core_from_config(
    paths: &PathResolver,
    runtime: &tokio::runtime::Handle,
    report: &mut Report,
) -> Result<(), String> {
    // The same typed formatter as ApplicationClient's persistence; no ad hoc YAML parser.
    let file = std::fs::File::open(paths.application_config_path()).map_err(|e| e.to_string())?;
    let config: NyanpasuAppConfig =
        crate::core::migration::modules::application::ApplicationFormat::default()
            .deserialize(file)
            .map_err(|e| e.to_string())?;
    let version = runtime.block_on(super::core_version::SidecarCoreVersionReader.read(config.core));
    add_core(report, config.core, version.map_err(|e| e.to_string()))
}
fn add_core(
    report: &mut Report,
    core: nyanpasu_config::application::ClashCore,
    version: Result<String, String>,
) -> Result<(), String> {
    let error = version.as_ref().err().cloned();
    report.core(CoreInfo {
        name: core.binary_name().into(),
        version: version.ok(),
        error: error.clone(),
    });
    error.map_or(Ok(()), Err)
}
fn write_core(
    archive: &mut Option<Archive>,
    report: &mut Report,
    export: CoreLogExport,
    fraction: &mut dyn FnMut(f32),
) -> Result<(), String> {
    let archive = archive.as_mut().ok_or("Archive unavailable")?;
    // The export is already capped and redacted inside the store operation.
    archive.core_logs(&export.lines, fraction)?;
    fraction(1.);
    if export.truncated {
        report.notes.push(format!(
            "Core logs truncated to the newest content within {LOG_CAP} uncompressed bytes."
        ));
    }
    Ok(())
}
/// Refreshes only the parent PID. The reporter remains responsive on its UI thread.
fn wait_for_parent(parent_pid: u32, fraction: &mut dyn FnMut(f32)) {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    if parent_pid == 0 {
        return;
    }
    let pid = Pid::from_u32(parent_pid);
    let mut system = System::new();
    loop {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        if system.process(pid).is_none() {
            break;
        }
        fraction(0.);
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Called on a blocking thread; all consumers use this same ordered run.
pub fn diagnose(
    paths: &PathResolver,
    client: Option<NyanpasuClient>,
    page: PageCapabilities,
    around: (String, String),
    parent_pid: u32,
    with_archive: bool,
    mut sink: impl FnMut(Event),
) {
    let runtime = tokio::runtime::Handle::current();
    let mut steps: Vec<(Step, Action<'_>)> = vec![
        (
            Step::Host,
            Box::new(|_, report, _| {
                let host = nyanpasu_diagnose::host::collect_host_info();
                let error = host.gpu_error.clone();
                report.host(&host);
                error.map_or(Ok(()), Err)
            }),
        ),
        (
            Step::Webview,
            Box::new(|_, report, _| {
                let webview = crate::window::collect_webview_info();
                let error = webview.version_error.clone();
                report.webview(webview);
                error.map_or(Ok(()), Err)
            }),
        ),
        (
            Step::Core,
            Box::new(|_, report, _| {
                if let Some(client) = &client {
                    let core = runtime
                        .block_on(client.get_app_config())
                        .map_err(|e| e.to_string())?
                        .core;
                    add_core(
                        report,
                        core,
                        runtime
                            .block_on(client.get_core_version(core))
                            .map_err(|e| e.to_string()),
                    )
                } else {
                    core_from_config(paths, &runtime, report)
                }
            }),
        ),
    ];
    if with_archive {
        steps.push((
            Step::AppLogs,
            Box::new(|archive, report, fraction| {
                archive.as_mut().ok_or("Archive unavailable")?.app_logs(
                    paths.app_logs_dir().as_std_path(),
                    report,
                    fraction,
                )
            }),
        ));
        steps.push((
            Step::CoreLogs,
            Box::new(|archive, report, fraction| {
                let export = if let Some(client) = &client {
                    runtime
                        .block_on(client.export_core_logs(home()))
                        .map_err(|e| e.to_string())?
                } else {
                    wait_for_parent(parent_pid, fraction);
                    let mut store = RedbCoreLogStore::open_for_export(
                        paths.app_logs_dir().join("core").into_std_path_buf(),
                    )
                    .map_err(|e| e.to_string())?;
                    store.export(home()).map_err(|e| e.to_string())?
                };
                write_core(archive, report, export, fraction)
            }),
        ));
    }
    let archive = with_archive.then(|| Archive::new(paths.app_logs_dir().as_std_path(), home()));
    let mut started = std::time::Instant::now();
    let mut current = None;
    nyanpasu_diagnose::run(steps, initial_report(around, page), archive, |event| {
        // Useful for CLI measurements and debugging; never log collected content.
        if let Event::Progress { step, fraction } = &event {
            if current != Some(*step) {
                current = Some(*step);
                started = std::time::Instant::now();
            }
            if *fraction == 1. {
                tracing::debug!(
                    ?step,
                    elapsed_ms = started.elapsed().as_millis(),
                    "diagnosis step"
                );
            }
        }
        sink(event);
    });
}

/// Archive-only collection shared by the save dialog and HTTP download.
pub async fn logs_archive(
    paths: PathResolver,
    client: NyanpasuClient,
) -> anyhow::Result<std::path::PathBuf> {
    super::blocking::join(
        tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Handle::current();
            let steps: Vec<(Step, Action<'_>)> = vec![
                (
                    Step::AppLogs,
                    Box::new(|archive, report, fraction| {
                        archive.as_mut().ok_or("Archive unavailable")?.app_logs(
                            paths.app_logs_dir().as_std_path(),
                            report,
                            fraction,
                        )
                    }),
                ),
                (
                    Step::CoreLogs,
                    Box::new(|archive, report, fraction| {
                        let export = runtime
                            .block_on(client.export_core_logs(home()))
                            .map_err(|e| e.to_string())?;
                        write_core(archive, report, export, fraction)
                    }),
                ),
            ];
            let mut result = None;
            nyanpasu_diagnose::run(
                steps,
                Report::default(),
                Some(Archive::new(paths.app_logs_dir().as_std_path(), home())),
                |event| {
                    if let Event::Finished { archive, .. } = event {
                        result = archive;
                    }
                },
            );
            result.ok_or_else(|| anyhow::anyhow!("Could not create diagnostics archive"))
        })
        .await,
    )
}

/// Opens a local reporter and returns as soon as the child is spawned.
pub fn report_issue(
    paths: PathResolver,
    client: NyanpasuClient,
    page: PageCapabilities,
) -> anyhow::Result<()> {
    let mut command = std::process::Command::new(tauri::utils::platform::current_exe()?);
    command
        .args([
            "report-issue",
            "--language",
            client.app_config_snapshot().language.as_str(),
        ])
        .stdin(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take();
    tokio::task::spawn_blocking(move || {
        diagnose(
            &paths,
            Some(client),
            page,
            Default::default(),
            0,
            true,
            |event| {
                if let Some(writer) = &mut stdin {
                    let result = serde_json::to_writer(&mut *writer, &event)
                        .map_err(std::io::Error::other)
                        .and_then(|()| writer.write_all(b"\n"))
                        .and_then(|()| writer.flush());
                    if result.is_err() {
                        stdin = None;
                    }
                }
            },
        );
        drop(stdin);
        // Reap even when the user closes the window early.
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Roots are explicitly staged copies inside the worktree; never mutate real user logs.
    #[test]
    #[ignore = "opt-in diagnosis measurement over injected roots"]
    fn measure_diagnose() {
        let config = std::env::var("NYANPASU_DIAGNOSE_CONFIG").expect("injected config root");
        let data = std::env::var("NYANPASU_DIAGNOSE_DATA").expect("injected data root");
        let paths = PathResolver::with_base_dirs(config.into(), data.into());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();
        let mut current = None;
        let mut started = std::time::Instant::now();
        diagnose(
            &paths,
            None,
            Default::default(),
            Default::default(),
            0,
            true,
            |event| match event {
                Event::Progress { step, fraction } => {
                    if current != Some(step) {
                        if let Some(previous) = current {
                            println!(
                                "{previous:?}: {:.3} ms",
                                started.elapsed().as_secs_f64() * 1000.
                            );
                        }
                        current = Some(step);
                        started = std::time::Instant::now();
                    }
                    let _ = fraction;
                }
                Event::Finished {
                    archive, problems, ..
                } => {
                    println!(
                        "{:?}: {:.3} ms (including archive finalization)",
                        current.unwrap(),
                        started.elapsed().as_secs_f64() * 1000.
                    );
                    let path = archive.expect("diagnostic archive");
                    println!(
                        "archive_bytes={} archive={} problems={}",
                        std::fs::metadata(&path).unwrap().len(),
                        path.display(),
                        problems.len()
                    );
                    for problem in problems {
                        println!("{:?}: {}", problem.step, problem.message);
                    }
                }
                _ => {}
            },
        );
    }
    #[test]
    fn parent_wait_proceeds_after_a_live_child_exits() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "utils::collect::tests::parent_wait_dummy",
            "--ignored",
        ]);
        let child = command.spawn().unwrap();
        let mut child = Some(child);
        let mut polls = 0;
        wait_for_parent(child.as_ref().unwrap().id(), &mut |fraction| {
            assert_eq!(fraction, 0.);
            polls += 1;
            if let Some(mut child) = child.take() {
                child.kill().unwrap();
                child.wait().unwrap();
            }
        });
        assert!(polls >= 1);
    }
    #[test]
    #[ignore = "live-process fixture for parent wait"]
    fn parent_wait_dummy() {
        std::thread::park();
    }
}
