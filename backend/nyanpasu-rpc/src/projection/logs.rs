//! Explicit application-boundary conversions for logs.

use nyanpasu_core::client::{
    frontend_events::{
        FrontendErrorCause as DomainFrontendErrorCause, FrontendEvent as DomainFrontendEvent,
        FrontendEventBatch as DomainFrontendEventBatch,
        FrontendEventKind as DomainFrontendEventKind,
        FrontendEventLevel as DomainFrontendEventLevel,
    },
    logs::LogSource as DomainLogSource,
};
use nyanpasu_observers::logs::{
    CoreLogCursor as DomainCoreLogCursor, CoreLogDirection as DomainCoreLogDirection,
    CoreLogError as DomainCoreLogError, CoreLogPage as DomainCoreLogPage,
    CoreLogQuery as DomainCoreLogQuery, CoreLogRecord as DomainCoreLogRecord,
    CoreLogRow as DomainCoreLogRow, CoreLogSource as DomainCoreLogSource,
    CoreLogStatus as DomainCoreLogStatus,
};
use nyanpasu_rpc_protocol::wire;

use super::{IntoApplication, IntoWire};

impl IntoWire for DomainLogSource {
    type Wire = wire::logs::LogSource;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainLogSource::App => wire::logs::LogSource::App,
            DomainLogSource::Service => wire::logs::LogSource::Service,
        }
    }
}

impl IntoApplication for wire::logs::LogSource {
    type Application = DomainLogSource;

    fn into_application(self) -> Self::Application {
        match self {
            wire::logs::LogSource::App => DomainLogSource::App,
            wire::logs::LogSource::Service => DomainLogSource::Service,
        }
    }
}

impl IntoWire for DomainFrontendEventKind {
    type Wire = wire::logs::FrontendEventKind;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainFrontendEventKind::Console => wire::logs::FrontendEventKind::Console,
            DomainFrontendEventKind::UncaughtError => wire::logs::FrontendEventKind::UncaughtError,
            DomainFrontendEventKind::UnhandledRejection => {
                wire::logs::FrontendEventKind::UnhandledRejection
            }
            DomainFrontendEventKind::ReactUncaught => wire::logs::FrontendEventKind::ReactUncaught,
            DomainFrontendEventKind::ReactCaught => wire::logs::FrontendEventKind::ReactCaught,
            DomainFrontendEventKind::ReactRecoverable => {
                wire::logs::FrontendEventKind::ReactRecoverable
            }
        }
    }
}

impl IntoApplication for wire::logs::FrontendEventKind {
    type Application = DomainFrontendEventKind;

    fn into_application(self) -> Self::Application {
        match self {
            wire::logs::FrontendEventKind::Console => DomainFrontendEventKind::Console,
            wire::logs::FrontendEventKind::UncaughtError => DomainFrontendEventKind::UncaughtError,
            wire::logs::FrontendEventKind::UnhandledRejection => {
                DomainFrontendEventKind::UnhandledRejection
            }
            wire::logs::FrontendEventKind::ReactUncaught => DomainFrontendEventKind::ReactUncaught,
            wire::logs::FrontendEventKind::ReactCaught => DomainFrontendEventKind::ReactCaught,
            wire::logs::FrontendEventKind::ReactRecoverable => {
                DomainFrontendEventKind::ReactRecoverable
            }
        }
    }
}

impl IntoWire for DomainFrontendEventLevel {
    type Wire = wire::logs::FrontendEventLevel;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainFrontendEventLevel::Warning => wire::logs::FrontendEventLevel::Warning,
            DomainFrontendEventLevel::Error => wire::logs::FrontendEventLevel::Error,
        }
    }
}

impl IntoApplication for wire::logs::FrontendEventLevel {
    type Application = DomainFrontendEventLevel;

    fn into_application(self) -> Self::Application {
        match self {
            wire::logs::FrontendEventLevel::Warning => DomainFrontendEventLevel::Warning,
            wire::logs::FrontendEventLevel::Error => DomainFrontendEventLevel::Error,
        }
    }
}

impl IntoWire for DomainFrontendErrorCause {
    type Wire = wire::logs::FrontendErrorCause;

    fn into_wire(self) -> Self::Wire {
        wire::logs::FrontendErrorCause {
            name: self.name,
            message: self.message,
            stack: self.stack,
        }
    }
}

impl IntoApplication for wire::logs::FrontendErrorCause {
    type Application = DomainFrontendErrorCause;

    fn into_application(self) -> Self::Application {
        DomainFrontendErrorCause {
            name: self.name,
            message: self.message,
            stack: self.stack,
        }
    }
}

impl IntoWire for DomainFrontendEvent {
    type Wire = wire::logs::FrontendEvent;

    fn into_wire(self) -> Self::Wire {
        wire::logs::FrontendEvent {
            kind: self.kind.into_wire(),
            level: self.level.into_wire(),
            message: self.message,
            error_name: self.error_name,
            stack: self.stack,
            causes: self
                .causes
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            component_stack: self.component_stack,
            fingerprint: self.fingerprint,
            count: self.count,
            first_seen_ms: self.first_seen_ms,
            last_seen_ms: self.last_seen_ms,
            route: self.route,
        }
    }
}

impl IntoApplication for wire::logs::FrontendEvent {
    type Application = DomainFrontendEvent;

    fn into_application(self) -> Self::Application {
        DomainFrontendEvent {
            kind: self.kind.into_application(),
            level: self.level.into_application(),
            message: self.message,
            error_name: self.error_name,
            stack: self.stack,
            causes: self
                .causes
                .into_iter()
                .map(|value| value.into_application())
                .collect(),
            component_stack: self.component_stack,
            fingerprint: self.fingerprint,
            count: self.count,
            first_seen_ms: self.first_seen_ms,
            last_seen_ms: self.last_seen_ms,
            route: self.route,
        }
    }
}

impl IntoWire for DomainFrontendEventBatch {
    type Wire = wire::logs::FrontendEventBatch;

    fn into_wire(self) -> Self::Wire {
        wire::logs::FrontendEventBatch {
            events: self
                .events
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            dropped: self.dropped,
        }
    }
}

impl IntoApplication for wire::logs::FrontendEventBatch {
    type Application = DomainFrontendEventBatch;

    fn into_application(self) -> Self::Application {
        DomainFrontendEventBatch {
            events: self
                .events
                .into_iter()
                .map(|value| value.into_application())
                .collect(),
            dropped: self.dropped,
        }
    }
}

impl IntoWire for DomainCoreLogError {
    type Wire = wire::logs::CoreLogError;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainCoreLogError::InvalidRequest => wire::logs::CoreLogError::InvalidRequest,
            DomainCoreLogError::CursorExpired => wire::logs::CoreLogError::CursorExpired,
            DomainCoreLogError::RecordGone => wire::logs::CoreLogError::RecordGone,
            DomainCoreLogError::TooLarge => wire::logs::CoreLogError::TooLarge,
            DomainCoreLogError::Unavailable(value0) => {
                wire::logs::CoreLogError::Unavailable(value0)
            }
        }
    }
}

impl IntoWire for DomainCoreLogCursor {
    type Wire = wire::logs::CoreLogCursor;

    fn into_wire(self) -> Self::Wire {
        wire::logs::CoreLogCursor {
            generation: self.generation,
            sequence: self.sequence,
        }
    }
}

impl IntoApplication for wire::logs::CoreLogCursor {
    type Application = DomainCoreLogCursor;

    fn into_application(self) -> Self::Application {
        DomainCoreLogCursor {
            generation: self.generation,
            sequence: self.sequence,
        }
    }
}

impl IntoWire for DomainCoreLogSource {
    type Wire = wire::logs::CoreLogSource;

    fn into_wire(self) -> Self::Wire {
        wire::logs::CoreLogSource {
            capture: self.capture,
            instance_id: self.instance_id,
            core_kind: self.core_kind,
        }
    }
}

impl IntoWire for DomainCoreLogRecord {
    type Wire = wire::logs::CoreLogRecord;

    fn into_wire(self) -> Self::Wire {
        wire::logs::CoreLogRecord {
            source: self.source.into_wire(),
            received_at: self.received_at,
            time: self.time,
            log_type: self.log_type,
            payload: self.payload,
        }
    }
}

impl IntoWire for DomainCoreLogRow {
    type Wire = wire::logs::CoreLogRow;

    fn into_wire(self) -> Self::Wire {
        wire::logs::CoreLogRow {
            id: self.id.into_wire(),
            record: self.record.into_wire(),
            truncated: self.truncated,
        }
    }
}

impl IntoWire for DomainCoreLogDirection {
    type Wire = wire::logs::CoreLogDirection;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainCoreLogDirection::Latest => wire::logs::CoreLogDirection::Latest,
            DomainCoreLogDirection::Before => wire::logs::CoreLogDirection::Before,
            DomainCoreLogDirection::After => wire::logs::CoreLogDirection::After,
        }
    }
}

impl IntoApplication for wire::logs::CoreLogDirection {
    type Application = DomainCoreLogDirection;

    fn into_application(self) -> Self::Application {
        match self {
            wire::logs::CoreLogDirection::Latest => DomainCoreLogDirection::Latest,
            wire::logs::CoreLogDirection::Before => DomainCoreLogDirection::Before,
            wire::logs::CoreLogDirection::After => DomainCoreLogDirection::After,
        }
    }
}

impl IntoWire for DomainCoreLogQuery {
    type Wire = wire::logs::CoreLogQuery;

    fn into_wire(self) -> Self::Wire {
        wire::logs::CoreLogQuery {
            direction: self.direction.into_wire(),
            cursor: self.cursor.map(|value| value.into_wire()),
            level: self.level,
            keyword: self.keyword,
            limit: self.limit,
        }
    }
}

impl IntoApplication for wire::logs::CoreLogQuery {
    type Application = DomainCoreLogQuery;

    fn into_application(self) -> Self::Application {
        DomainCoreLogQuery {
            direction: self.direction.into_application(),
            cursor: self.cursor.map(|value| value.into_application()),
            level: self.level,
            keyword: self.keyword,
            limit: self.limit,
        }
    }
}

impl IntoWire for DomainCoreLogStatus {
    type Wire = wire::logs::CoreLogStatus;

    fn into_wire(self) -> Self::Wire {
        wire::logs::CoreLogStatus {
            generation: self.generation,
            version: self.version,
            first: self.first.map(|value| value.into_wire()),
            head: self.head.map(|value| value.into_wire()),
            bytes: self.bytes,
            error: self.error,
            discarded: self.discarded,
        }
    }
}

impl IntoWire for DomainCoreLogPage {
    type Wire = wire::logs::CoreLogPage;

    fn into_wire(self) -> Self::Wire {
        wire::logs::CoreLogPage {
            rows: self
                .rows
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            cursor: self.cursor.map(|value| value.into_wire()),
            more: self.more,
            status: self.status.into_wire(),
        }
    }
}
