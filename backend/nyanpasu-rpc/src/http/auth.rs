//! Who may use a router-mode listener: one-time tickets and the sessions they
//! become. The listener owns one [`WebAuth`] and hands the issuing half to the
//! shell's composition root as a [`WebTicketIssuer`]; nothing else reaches it.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use uuid::Uuid;

use crate::web_ticket::{WebTicket, WebTicketIssuer};

/// How long a ticket can be exchanged after it is issued.
const TICKET_TTL: Duration = Duration::from_secs(60);
/// Outstanding tickets; issuing more evicts the oldest.
const MAX_TICKETS: usize = 16;
/// Live sessions; signing in beyond this evicts the oldest.
const MAX_SESSIONS: usize = 8;

/// The tickets and sessions as plain data. Time is a parameter, so expiry is
/// tested without waiting.
#[derive(Default)]
struct AuthBook {
    tickets: HashMap<String, Instant>,
    sessions: VecDeque<Uuid>,
}

impl AuthBook {
    fn add_ticket(&mut self, ticket: String, now: Instant) {
        self.tickets
            .retain(|_, issued| now.saturating_duration_since(*issued) < TICKET_TTL);
        if self.tickets.len() >= MAX_TICKETS
            && let Some(oldest) = self
                .tickets
                .iter()
                .min_by_key(|(_, issued)| **issued)
                .map(|(ticket, _)| ticket.clone())
        {
            self.tickets.remove(&oldest);
        }
        self.tickets.insert(ticket, now);
    }

    /// Consumes `ticket`, valid or not, and opens `session` when it was
    /// issued less than the TTL ago.
    fn redeem(&mut self, ticket: &str, session: Uuid, now: Instant) -> bool {
        let Some(issued) = self.tickets.remove(ticket) else {
            return false;
        };
        if now.saturating_duration_since(issued) >= TICKET_TTL {
            return false;
        }
        if self.sessions.len() >= MAX_SESSIONS {
            self.sessions.pop_front();
        }
        self.sessions.push_back(session);
        true
    }

    fn is_session(&self, session: Uuid) -> bool {
        self.sessions.contains(&session)
    }
}

/// A router-mode listener's authentication state and the port it listens on.
///
/// The book sits behind a lock because the listener's request middleware and
/// the `issue_web_ticket` command both reach it synchronously and neither holds
/// the guard across an `.await`; it is state of this listener alone, created
/// with it and dropped with it.
pub(super) struct WebAuth {
    book: Mutex<AuthBook>,
    port: u16,
}

impl WebAuth {
    pub(super) fn new(port: u16) -> Self {
        Self {
            book: Mutex::default(),
            port,
        }
    }

    /// Opens a session for a valid, unused ticket.
    pub(super) fn redeem(&self, ticket: &str) -> Option<Uuid> {
        let session = Uuid::new_v4();
        self.book
            .lock()
            .redeem(ticket, session, Instant::now())
            .then_some(session)
    }

    pub(super) fn is_session(&self, session: Uuid) -> bool {
        self.book.lock().is_session(session)
    }
}

impl WebTicketIssuer for WebAuth {
    fn issue(&self) -> WebTicket {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("the operating system provides random bytes");
        let ticket = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.book.lock().add_ticket(ticket.clone(), Instant::now());
        WebTicket {
            path: format!("/?ticket={ticket}"),
            port: self.port,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64, start: Instant) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn a_ticket_opens_one_session_once() {
        let start = Instant::now();
        let mut book = AuthBook::default();
        let session = Uuid::new_v4();
        book.add_ticket("t".into(), start);

        assert!(!book.is_session(session));
        assert!(book.redeem("t", session, at(1, start)));
        assert!(book.is_session(session));
        assert!(!book.redeem("t", Uuid::new_v4(), at(2, start)));
    }

    #[test]
    fn an_unknown_ticket_opens_nothing_and_spares_the_real_one() {
        let start = Instant::now();
        let mut book = AuthBook::default();
        book.add_ticket("t".into(), start);
        let session = Uuid::new_v4();

        assert!(!book.redeem("other", session, start));
        assert!(!book.is_session(session));
        assert!(book.redeem("t", session, start));
    }

    #[test]
    fn a_ticket_expires_and_the_late_attempt_consumes_it() {
        let start = Instant::now();
        let mut book = AuthBook::default();
        book.add_ticket("t".into(), start);
        let session = Uuid::new_v4();

        assert!(!book.redeem("t", session, at(60, start)));
        assert!(!book.is_session(session));
        assert!(!book.redeem("t", session, at(61, start)));
    }

    #[test]
    fn a_ticket_just_inside_the_ttl_still_works() {
        let start = Instant::now();
        let mut book = AuthBook::default();
        book.add_ticket("t".into(), start);

        assert!(book.redeem("t", Uuid::new_v4(), at(59, start)));
    }

    #[test]
    fn tickets_and_sessions_are_capped_and_evict_the_oldest() {
        let start = Instant::now();
        let mut book = AuthBook::default();
        for index in 0..=MAX_TICKETS {
            book.add_ticket(index.to_string(), at(index as u64, start));
        }
        assert_eq!(book.tickets.len(), MAX_TICKETS);
        assert!(!book.redeem("0", Uuid::new_v4(), at(20, start)));
        assert!(book.redeem("1", Uuid::new_v4(), at(20, start)));

        let sessions = (0..=MAX_SESSIONS)
            .map(|_| Uuid::new_v4())
            .collect::<Vec<_>>();
        for (index, session) in sessions.iter().enumerate() {
            book.add_ticket(format!("s{index}"), at(30, start));
            assert!(book.redeem(&format!("s{index}"), *session, at(31, start)));
        }
        assert!(!book.is_session(sessions[0]));
        assert!(
            sessions[1..]
                .iter()
                .all(|session| book.is_session(*session))
        );
    }

    #[test]
    fn expired_tickets_are_dropped_when_a_new_one_is_issued() {
        let start = Instant::now();
        let mut book = AuthBook::default();
        book.add_ticket("old".into(), start);
        book.add_ticket("new".into(), at(61, start));

        assert_eq!(book.tickets.len(), 1);
        assert!(book.tickets.contains_key("new"));
    }

    #[test]
    fn the_issuer_mints_128_bit_hex_tickets_that_redeem_once() {
        let auth = WebAuth::new(7899);
        let first = auth.issue();
        let second = auth.issue();

        assert_eq!(first.port, 7899);
        let ticket = first.path.strip_prefix("/?ticket=").unwrap();
        assert_eq!(ticket.len(), 32);
        assert!(ticket.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first.path, second.path);
        let session = auth.redeem(ticket).unwrap();
        assert!(auth.is_session(session));
        assert!(auth.redeem(ticket).is_none());
    }
}
