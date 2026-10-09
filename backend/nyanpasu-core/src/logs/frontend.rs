//! Frontend warnings and errors written to the application log.
//!
//! The frontend deduplicates, throttles and batches; this side only bounds a
//! single request and keeps no state between requests. Log rotation bounds
//! the disk use of everything that passes.

/// The target every frontend event is logged under. It shares the app's
/// configured log level through the `clash_nyanpasu` prefix.
pub const FRONTEND_LOG_TARGET: &str = "clash_nyanpasu::frontend";

const MAX_EVENTS: usize = 32;
const MAX_CAUSES: usize = 3;
const MAX_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_STACK_BYTES: usize = 16 * 1024;
const MAX_SHORT_BYTES: usize = 512;
const MAX_FINGERPRINT_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FrontendEventKind {
    Console,
    UncaughtError,
    UnhandledRejection,
    ReactUncaught,
    ReactCaught,
    ReactRecoverable,
}

impl FrontendEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::UncaughtError => "uncaught_error",
            Self::UnhandledRejection => "unhandled_rejection",
            Self::ReactUncaught => "react_uncaught",
            Self::ReactCaught => "react_caught",
            Self::ReactRecoverable => "react_recoverable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FrontendEventLevel {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FrontendErrorCause {
    pub name: Option<String>,
    pub message: String,
    pub stack: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FrontendEvent {
    pub kind: FrontendEventKind,
    pub level: FrontendEventLevel,
    pub message: String,
    pub error_name: Option<String>,
    pub stack: Option<String>,
    /// The `error.cause` chain, outermost first.
    pub causes: Vec<FrontendErrorCause>,
    /// React's component stack, for the `react_*` kinds.
    pub component_stack: Option<String>,
    pub fingerprint: String,
    /// How often the fingerprint occurred within the frontend's dedupe window.
    pub count: u32,
    /// Client clock, Unix milliseconds. The log line's own timestamp is when
    /// the backend wrote it.
    pub first_seen_ms: Option<f64>,
    pub last_seen_ms: Option<f64>,
    /// The route path, without query parameters.
    pub route: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FrontendEventBatch {
    pub events: Vec<FrontendEvent>,
    /// Events the frontend dropped since its previous batch.
    pub dropped: u32,
}

/// A frontend event within the per-request bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct SanitizedEvent {
    pub event: FrontendEvent,
    /// Whether any field was cut to its bound.
    pub truncated: bool,
}

#[cfg_attr(test, mockall::automock)]
pub trait FrontendLogSink: Send + Sync + 'static {
    fn write(&self, owner: &str, event: &SanitizedEvent);
    fn dropped(&self, owner: &str, count: u32);
}

/// Writes frontend events through the process `tracing` subscriber, which
/// owns the application log files.
pub struct TracingFrontendLogSink;

impl FrontendLogSink for TracingFrontendLogSink {
    fn write(&self, owner: &str, sanitized: &SanitizedEvent) {
        let event = &sanitized.event;
        let causes = (!event.causes.is_empty())
            .then(|| serde_json::to_string(&event.causes).expect("causes serialize"));
        macro_rules! emit {
            ($level:expr) => {
                tracing::event!(
                    target: FRONTEND_LOG_TARGET,
                    $level,
                    owner,
                    kind = event.kind.as_str(),
                    fingerprint = %event.fingerprint,
                    count = event.count,
                    route = %event.route,
                    error_name = event.error_name.as_deref(),
                    stack = event.stack.as_deref(),
                    causes = causes.as_deref(),
                    component_stack = event.component_stack.as_deref(),
                    first_seen_ms = event.first_seen_ms.map(|ms| ms as i64),
                    last_seen_ms = event.last_seen_ms.map(|ms| ms as i64),
                    truncated = sanitized.truncated,
                    "{}",
                    event.message
                )
            };
        }
        match event.level {
            FrontendEventLevel::Warning => emit!(tracing::Level::WARN),
            FrontendEventLevel::Error => emit!(tracing::Level::ERROR),
        }
    }

    fn dropped(&self, owner: &str, count: u32) {
        tracing::warn!(
            target: FRONTEND_LOG_TARGET,
            owner,
            dropped = count,
            "frontend dropped {count} events"
        );
    }
}

/// Bounds one request: the event count, every string, and the cause chain.
/// Returns the events to log and how many were dropped in total.
pub fn sanitize(batch: FrontendEventBatch) -> (Vec<SanitizedEvent>, u32) {
    let excess = batch.events.len().saturating_sub(MAX_EVENTS);
    let dropped = batch
        .dropped
        .saturating_add(u32::try_from(excess).unwrap_or(u32::MAX));
    let events = batch
        .events
        .into_iter()
        .take(MAX_EVENTS)
        .map(sanitize_event)
        .collect();
    (events, dropped)
}

fn sanitize_event(mut event: FrontendEvent) -> SanitizedEvent {
    let mut truncated = false;
    let mut bound = |value: &mut String, max: usize| truncated |= truncate(value, max);

    bound(&mut event.message, MAX_MESSAGE_BYTES);
    bound(&mut event.route, MAX_SHORT_BYTES);
    if let Some(name) = &mut event.error_name {
        bound(name, MAX_SHORT_BYTES);
    }
    if let Some(stack) = &mut event.stack {
        bound(stack, MAX_STACK_BYTES);
    }
    if let Some(stack) = &mut event.component_stack {
        bound(stack, MAX_STACK_BYTES);
    }
    if event.causes.len() > MAX_CAUSES {
        event.causes.truncate(MAX_CAUSES);
        truncated = true;
    }
    for cause in &mut event.causes {
        truncated |= truncate(&mut cause.message, MAX_MESSAGE_BYTES);
        if let Some(name) = &mut cause.name {
            truncated |= truncate(name, MAX_SHORT_BYTES);
        }
        if let Some(stack) = &mut cause.stack {
            truncated |= truncate(stack, MAX_STACK_BYTES);
        }
    }
    if !valid_fingerprint(&event.fingerprint) {
        event.fingerprint = fallback_fingerprint(&event);
    }
    event.count = event.count.max(1);
    for ms in [&mut event.first_seen_ms, &mut event.last_seen_ms] {
        *ms = ms.filter(|ms| ms.is_finite());
    }
    SanitizedEvent { event, truncated }
}

/// Cuts `value` to at most `max` bytes on a character boundary.
fn truncate(value: &mut String, max: usize) -> bool {
    if value.len() <= max {
        return false;
    }
    let end = value.floor_char_boundary(max);
    value.truncate(end);
    true
}

fn valid_fingerprint(fingerprint: &str) -> bool {
    !fingerprint.is_empty()
        && fingerprint.len() <= MAX_FINGERPRINT_BYTES
        && fingerprint
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'_' || b == b'-')
}

/// FNV-1a over the kind and the bounded message, so an event with an
/// unusable fingerprint still groups with its repeats.
fn fallback_fingerprint(event: &FrontendEvent) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in event
        .kind
        .as_str()
        .bytes()
        .chain([0])
        .chain(event.message.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("backend-{hash:016x}")
}

/// Sanitizes `batch` and writes it to `sink` on behalf of `owner`.
pub fn report(sink: &dyn FrontendLogSink, owner: &str, batch: FrontendEventBatch) {
    let (events, dropped) = sanitize(batch);
    for event in &events {
        sink.write(owner, event);
    }
    if dropped > 0 {
        sink.dropped(owner, dropped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn event(message: &str) -> FrontendEvent {
        FrontendEvent {
            kind: FrontendEventKind::Console,
            level: FrontendEventLevel::Error,
            message: message.into(),
            error_name: Some("TypeError".into()),
            stack: Some("TypeError: x\n    at f (app.js:1:2)".into()),
            causes: vec![],
            component_stack: None,
            fingerprint: "console-abc123".into(),
            count: 3,
            first_seen_ms: Some(1_700_000_000_000.0),
            last_seen_ms: Some(1_700_000_000_500.0),
            route: "/main/dashboard".into(),
        }
    }

    #[test]
    fn events_within_bounds_pass_unchanged() {
        let (events, dropped) = sanitize(FrontendEventBatch {
            events: vec![event("boom")],
            dropped: 0,
        });
        assert_eq!(dropped, 0);
        assert_eq!(
            events,
            vec![SanitizedEvent {
                event: event("boom"),
                truncated: false
            }]
        );
    }

    #[test]
    fn events_beyond_the_batch_bound_count_as_dropped() {
        let (events, dropped) = sanitize(FrontendEventBatch {
            events: (0..40).map(|i| event(&i.to_string())).collect(),
            dropped: 5,
        });
        assert_eq!(events.len(), MAX_EVENTS);
        assert_eq!(events.last().unwrap().event.message, "31");
        assert_eq!(dropped, 5 + 8);
    }

    #[test]
    fn long_fields_are_cut_on_a_character_boundary() {
        let mut long = event(&"é".repeat(MAX_MESSAGE_BYTES));
        long.stack = Some("s".repeat(MAX_STACK_BYTES + 1));
        long.route = "/".repeat(MAX_SHORT_BYTES + 1);
        long.causes = (0..5)
            .map(|_| FrontendErrorCause {
                name: None,
                message: "c".into(),
                stack: None,
            })
            .collect();
        let (events, _) = sanitize(FrontendEventBatch {
            events: vec![long],
            dropped: 0,
        });
        let SanitizedEvent { event, truncated } = &events[0];
        assert!(truncated);
        assert_eq!(event.message.len(), MAX_MESSAGE_BYTES);
        assert!(event.message.chars().all(|c| c == 'é'));
        assert_eq!(event.stack.as_ref().unwrap().len(), MAX_STACK_BYTES);
        assert_eq!(event.route.len(), MAX_SHORT_BYTES);
        assert_eq!(event.causes.len(), MAX_CAUSES);

        let mut odd = event.clone();
        odd.message = format!("a{}", "é".repeat(MAX_MESSAGE_BYTES));
        let (events, _) = sanitize(FrontendEventBatch {
            events: vec![odd],
            dropped: 0,
        });
        // One byte short of the bound: the next two-byte character does not fit.
        assert_eq!(events[0].event.message.len(), MAX_MESSAGE_BYTES - 1);
    }

    #[test]
    fn an_unusable_fingerprint_is_replaced_by_a_stable_one() {
        let mut first = event("boom");
        first.fingerprint = "Not Valid!".into();
        let mut second = event("boom");
        second.fingerprint = String::new();
        let (events, _) = sanitize(FrontendEventBatch {
            events: vec![first, second, event("other")],
            dropped: 0,
        });
        assert!(events[0].event.fingerprint.starts_with("backend-"));
        assert_eq!(events[0].event.fingerprint, events[1].event.fingerprint);
        assert_eq!(events[2].event.fingerprint, "console-abc123");
        assert!(valid_fingerprint(&events[0].event.fingerprint));
    }

    #[test]
    fn counts_and_clock_values_are_normalized() {
        let mut odd = event("boom");
        odd.count = 0;
        odd.first_seen_ms = Some(f64::NAN);
        odd.last_seen_ms = Some(f64::INFINITY);
        let (events, _) = sanitize(FrontendEventBatch {
            events: vec![odd],
            dropped: 0,
        });
        assert_eq!(events[0].event.count, 1);
        assert_eq!(events[0].event.first_seen_ms, None);
        assert_eq!(events[0].event.last_seen_ms, None);
    }

    #[test]
    fn report_writes_each_event_and_one_dropped_record_for_the_owner() {
        let mut sink = MockFrontendLogSink::new();
        sink.expect_write()
            .withf(|owner, event| owner == "main" && event.event.message == "boom")
            .times(1)
            .return_const(());
        sink.expect_dropped()
            .withf(|owner, count| owner == "main" && *count == 2)
            .times(1)
            .return_const(());
        report(
            &sink,
            "main",
            FrontendEventBatch {
                events: vec![event("boom")],
                dropped: 2,
            },
        );

        let mut quiet = MockFrontendLogSink::new();
        quiet.expect_write().never();
        quiet.expect_dropped().never();
        report(
            &quiet,
            "main",
            FrontendEventBatch {
                events: vec![],
                dropped: 0,
            },
        );
    }

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The tracing sink writes JSON lines under the frontend target, at the
    /// event's level, with the fields the analysis relies on.
    #[test]
    fn tracing_sink_writes_structured_json_lines() {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::TRACE)
            .with_writer({
                let buffer = buffer.clone();
                move || buffer.clone()
            })
            .finish();
        let mut warning = event("careful\nsecond line");
        warning.level = FrontendEventLevel::Warning;
        warning.causes = vec![FrontendErrorCause {
            name: Some("Error".into()),
            message: "root".into(),
            stack: None,
        }];
        tracing::subscriber::with_default(subscriber, || {
            let sink = TracingFrontendLogSink;
            sink.write(
                "main",
                &SanitizedEvent {
                    event: event("boom"),
                    truncated: false,
                },
            );
            sink.write(
                "browser",
                &SanitizedEvent {
                    event: warning,
                    truncated: true,
                },
            );
            sink.dropped("main", 7);
        });

        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<serde_json::Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 3, "{output}");
        for line in &lines {
            assert_eq!(line["target"], FRONTEND_LOG_TARGET);
        }

        assert_eq!(lines[0]["level"], "ERROR");
        let fields = &lines[0]["fields"];
        assert_eq!(fields["message"], "boom");
        assert_eq!(fields["owner"], "main");
        assert_eq!(fields["kind"], "console");
        assert_eq!(fields["fingerprint"], "console-abc123");
        assert_eq!(fields["count"], 3);
        assert_eq!(fields["route"], "/main/dashboard");
        assert_eq!(fields["error_name"], "TypeError");
        assert!(fields["stack"].as_str().unwrap().contains("app.js:1:2"));
        assert_eq!(fields["first_seen_ms"], 1_700_000_000_000_i64);
        assert_eq!(fields["truncated"], false);
        assert!(fields.get("causes").is_none());

        assert_eq!(lines[1]["level"], "WARN");
        let fields = &lines[1]["fields"];
        assert_eq!(fields["message"], "careful\nsecond line");
        assert_eq!(fields["owner"], "browser");
        assert_eq!(fields["truncated"], true);
        let causes: Vec<FrontendErrorCause> =
            serde_json::from_str(fields["causes"].as_str().unwrap()).unwrap();
        assert_eq!(causes[0].message, "root");

        assert_eq!(lines[2]["level"], "WARN");
        assert_eq!(lines[2]["fields"]["dropped"], 7);
    }
}
