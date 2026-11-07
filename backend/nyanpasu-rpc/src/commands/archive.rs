//! Archive-only log collection for HTTP downloads and native save dialogs.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_diagnose::{
    Action, Event, Step,
    archive::{Archive, LOG_CAP},
    report::Report,
};
use nyanpasu_observers::logs::CoreLogExport;
use nyanpasu_paths::PathResolver;

fn home() -> Option<String> {
    dirs::home_dir().map(|path| path.to_string_lossy().into_owned())
}

/// Add the already-redacted core log export to a shared diagnostics archive.
pub fn write_core(
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

/// Archive-only collection shared by the save dialog and HTTP download.
pub async fn logs_archive(
    paths: PathResolver,
    client: NyanpasuClient,
) -> anyhow::Result<std::path::PathBuf> {
    nyanpasu_owner_support::blocking::join(
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
