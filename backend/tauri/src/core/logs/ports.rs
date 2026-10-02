use super::model::*;

/// Synchronous infrastructure operations run on a blocking thread. The actor
/// transfers this owner into that thread and awaits its return for each command.
pub trait CoreLogStore: Send + 'static {
    fn append(&mut self, records: &[Vec<u8>]) -> CoreLogResult<()>;
    fn query(&mut self, query: CoreLogQuery) -> CoreLogResult<CoreLogPage>;
    fn detail(&mut self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord>;
    fn clear(&mut self) -> CoreLogResult<()>;
    fn status(&mut self) -> CoreLogResult<CoreLogStatus>;
}

pub struct UnavailableCoreLogStore(pub String);

impl CoreLogStore for UnavailableCoreLogStore {
    fn append(&mut self, _: &[Vec<u8>]) -> CoreLogResult<()> {
        Err(CoreLogError::Unavailable(self.0.clone()))
    }
    fn query(&mut self, _: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        Err(CoreLogError::Unavailable(self.0.clone()))
    }
    fn detail(&mut self, _: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        Err(CoreLogError::Unavailable(self.0.clone()))
    }
    fn clear(&mut self) -> CoreLogResult<()> {
        Err(CoreLogError::Unavailable(self.0.clone()))
    }
    fn status(&mut self) -> CoreLogResult<CoreLogStatus> {
        Err(CoreLogError::Unavailable(self.0.clone()))
    }
}
