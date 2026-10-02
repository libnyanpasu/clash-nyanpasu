use super::*;
use std::{io::Write, sync::Arc};

#[derive(Clone, Default)]
struct Output(Arc<Mutex<Vec<u8>>>);

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Output {
    fn events(&self) -> Vec<serde_json::Value> {
        String::from_utf8(self.0.lock().clone())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn capture(test: impl FnOnce(&Output)) {
    let output = Output::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || test(&output));
}

#[test]
fn early_stages_are_replayed_once_with_the_original_monotonic_offsets() {
    capture(|output| {
        let timings = StartupTimings::new();
        drop(timings.stage("before_logging"));
        assert!(output.events().is_empty());
        timings.enable_logging();
        timings.enable_logging();
        let events = output.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["target"], "clash_nyanpasu::startup");
        let fields = &events[0]["fields"];
        assert_eq!(fields["stage"], "before_logging");
        let start = fields["start_ms"].as_f64().unwrap();
        let end = fields["end_ms"].as_f64().unwrap();
        let elapsed = fields["elapsed_ms"].as_f64().unwrap();
        assert!(end >= start && start >= 0.0);
        assert!((end - start - elapsed).abs() < 0.000_001);
    });
}

#[test]
fn scopes_include_nested_work_and_record_early_errors() {
    capture(|output| {
        let timings = StartupTimings::new();
        timings.enable_logging();
        let outer = timings.stage("outer");
        let fail = || -> Result<(), ()> {
            let _inner = timings.stage("inner");
            Err(())?;
            Ok(())
        };
        assert_eq!(fail(), Err(()));
        drop(outer);
        let events = output.events();
        assert_eq!(events.len(), 2);
        let inner = &events[0]["fields"];
        let outer = &events[1]["fields"];
        assert_eq!(inner["stage"], "inner");
        assert_eq!(outer["stage"], "outer");
        assert!(outer["start_ms"].as_f64().unwrap() <= inner["start_ms"].as_f64().unwrap());
        assert!(outer["end_ms"].as_f64().unwrap() >= inner["end_ms"].as_f64().unwrap());
    });
}

#[test]
fn milestones_are_deduplicated_and_details_do_not_claim_a_process_origin() {
    capture(|output| {
        let timings = StartupTimings::new();
        timings.enable_logging();
        timings.milestone("main_window_ready");
        timings.milestone("main_window_ready");
        drop(StartupStage::new("client.detail"));
        let events = output.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["fields"]["milestone"], "main_window_ready");
        assert_eq!(events[1]["fields"]["stage"], "client.detail");
        assert!(events[1]["fields"].get("start_ms").is_none());
        assert!(events[1]["fields"].get("end_ms").is_none());
    });
}
