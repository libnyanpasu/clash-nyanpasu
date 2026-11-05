//! The port a shell offers so that an authenticated local caller can sign a
//! browser in to the WebUI. The command `issue_web_ticket` consumes it; the
//! shell that serves the WebUI supplies the issuer when it composes the
//! registry, and a shell that serves none supplies [`WebTickets::unavailable`].

use std::sync::Arc;

use serde::Serialize;
use snafu::Snafu;

/// A one-time sign-in ticket for the WebUI listener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct WebTicket {
    /// The request target that exchanges the ticket for a session, for
    /// example `/?ticket=...`; it is valid on the WebUI's origin only.
    pub path: String,
    /// The TCP port the WebUI listens on. The caller supplies the hostname it
    /// reached this shell by.
    pub port: u16,
}

/// Mints tickets for the listener that serves the WebUI.
pub trait WebTicketIssuer: Send + Sync + 'static {
    /// A fresh ticket; it is single use and short lived.
    fn issue(&self) -> WebTicket;
}

#[derive(Debug, Snafu)]
#[snafu(display("the WebUI is not being served; enable the shell's HTTP listener"))]
pub struct WebUiUnavailable;

/// The WebUI ticket issuer, when the shell serves the WebUI.
#[derive(Clone)]
pub struct WebTickets(Option<Arc<dyn WebTicketIssuer>>);

impl WebTickets {
    pub fn new(issuer: Arc<dyn WebTicketIssuer>) -> Self {
        Self(Some(issuer))
    }

    /// For a shell that does not serve the WebUI.
    pub fn unavailable() -> Self {
        Self(None)
    }

    /// # Errors
    ///
    /// Fails when the shell serves no WebUI.
    pub fn issue(&self) -> Result<WebTicket, WebUiUnavailable> {
        self.0
            .as_ref()
            .map(|issuer| issuer.issue())
            .ok_or(WebUiUnavailable)
    }
}
