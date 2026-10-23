//! A synchronous diagnosis shared by desktop issue reports and crash children.
pub mod archive;
pub mod host;
pub mod report;
#[cfg(test)]
mod report_tests;
use archive::Archive;
use report::Report;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Host,
    Webview,
    Core,
    AppLogs,
    CoreLogs,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    Started {
        steps: Vec<Step>,
    },
    Progress {
        step: Step,
        fraction: f32,
    },
    Finished {
        /// The whole report, as the archive has it.
        report: String,
        /// Only the collected sections, for the issue's environment field.
        environment: String,
        archive: Option<PathBuf>,
        problems: Vec<Problem>,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Problem {
    pub step: Step,
    pub message: String,
}
pub type Action<'a> = Box<
    dyn FnOnce(&mut Option<Archive>, &mut Report, &mut dyn FnMut(f32)) -> Result<(), String> + 'a,
>;

/// Runs every step, even after an error. Dropping the event receiver does not cancel work.
pub fn run(
    steps: Vec<(Step, Action<'_>)>,
    mut report: Report,
    archive: Option<Result<Archive, String>>,
    mut sink: impl FnMut(Event),
) {
    sink(Event::Started {
        steps: steps.iter().map(|(step, _)| *step).collect(),
    });
    let mut problems = Vec::new();
    let mut archive = match archive.transpose() {
        Ok(archive) => archive,
        Err(message) => {
            problems.push(Problem {
                step: Step::AppLogs,
                message,
            });
            None
        }
    };
    for (step, action) in steps {
        sink(Event::Progress { step, fraction: 0. });
        if let Err(message) = action(&mut archive, &mut report, &mut |fraction| {
            sink(Event::Progress {
                step,
                fraction: fraction.clamp(0., 1.),
            })
        }) {
            problems.push(Problem { step, message });
        }
        sink(Event::Progress { step, fraction: 1. });
    }
    for problem in &problems {
        report
            .notes
            .push(format!("{:?}: {}", problem.step, problem.message));
    }
    let archive = archive.and_then(|archive| match archive.finish(&report.text()) {
        Ok(path) => Some(path),
        Err(message) => {
            report.notes.push(format!("Archive: {message}"));
            problems.push(Problem {
                step: Step::AppLogs,
                message,
            });
            None
        }
    });
    sink(Event::Finished {
        report: report.text(),
        environment: report.environment(),
        archive,
        problems,
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_step_does_not_stop_the_run_and_events_roundtrip() {
        let mut events = Vec::new();
        run(
            vec![
                (Step::Host, Box::new(|_, _, _| Err("missing".into()))),
                (
                    Step::Core,
                    Box::new(|_, report, fraction| {
                        fraction(0.5);
                        report.section("Core", [("version".into(), "ok".into())]);
                        Ok(())
                    }),
                ),
            ],
            Report::default(),
            None,
            |event| events.push(event),
        );
        assert!(matches!(events.first(), Some(Event::Started { .. })));
        match events.last().unwrap() {
            Event::Finished {
                report, problems, ..
            } => {
                assert_eq!(problems.len(), 1);
                assert!(report.contains("version: ok"));
                assert!(report.contains("Host: missing"));
            }
            _ => panic!(),
        }
        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let _: Event = serde_json::from_str(&json).unwrap();
        }
    }
}
