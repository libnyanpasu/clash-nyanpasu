use super::NyanpasuClient;
use crate::logs::frontend::{FrontendEventBatch, report};

impl NyanpasuClient {
    pub fn report_frontend_events(&self, owner: &str, batch: FrontendEventBatch) {
        report(self.inner.frontend_log.as_ref(), owner, batch);
    }
}
