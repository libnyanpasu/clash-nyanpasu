//! Memory budgets (§7.4). One total budget per store; the head, the catalog, the backend
//! cache, staging and queries all reserve from it. A reservation is returned when dropped.
//! A reservation that does not fit fails with `BudgetExceeded` instead of growing past the
//! budget.
//!
//! The profile gives one limit per memory field (§10.1), and each field gets its own
//! [`MemoryBudget`]. Queries share one through the client, which is the admission control
//! of §7.4: a query that does not fit is refused at the door, and no scheduler sits
//! behind it.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use snafu::{Snafu, ensure};

use crate::ErrorKind;

/// A byte budget, cloneable and shared: every clone reserves from the same counter.
#[derive(Clone, Debug)]
pub struct MemoryBudget {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    limit: u64,
    /// Bytes held by live reservations, never above `limit`. Every change happens under
    /// this lock, so a reserve that fails changes nothing and a waiter cannot miss the
    /// moment the count reaches zero.
    reserved: Mutex<u64>,
    /// Signalled when `reserved` becomes zero.
    released: Condvar,
}

impl MemoryBudget {
    pub fn new(limit: u64) -> Self {
        Self {
            shared: Arc::new(Shared {
                limit,
                reserved: Mutex::new(0),
                released: Condvar::new(),
            }),
        }
    }

    /// Reserves `bytes` for `what`, which names the owner in the error. The bytes return
    /// to the budget when the reservation is dropped.
    pub fn try_reserve(
        &self,
        what: &'static str,
        bytes: u64,
    ) -> Result<Reservation, BudgetExceeded> {
        self.take(what, bytes)?;
        Ok(Reservation {
            budget: self.clone(),
            what,
            bytes,
        })
    }

    /// The bytes held by live reservations.
    pub fn reserved(&self) -> u64 {
        *self.lock()
    }

    /// Blocks until no bytes are reserved. Recovery waits on it after revoking the
    /// snapshots, so that the old catalog and head are gone before the new ones are built
    /// (§7.4, §7.5).
    ///
    /// There is no timeout: the callers bound the wait themselves, because a query
    /// notices its revocation at the next column block or row batch and then drops its
    /// reservation.
    pub fn wait_until_released(&self) {
        let reserved = self.lock();
        drop(
            self.shared
                .released
                .wait_while(reserved, |reserved| *reserved != 0)
                .unwrap_or_else(PoisonError::into_inner),
        );
    }

    fn take(&self, what: &'static str, bytes: u64) -> Result<(), BudgetExceeded> {
        let mut reserved = self.lock();
        let available = self.shared.limit - *reserved;
        ensure!(
            bytes <= available,
            BudgetExceededSnafu {
                what,
                needed: bytes,
                available
            }
        );
        *reserved += bytes;
        Ok(())
    }

    fn give_back(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let mut reserved = self.lock();
        *reserved -= bytes;
        if *reserved == 0 {
            self.shared.released.notify_all();
        }
    }

    /// A poisoned lock still holds a consistent count: nothing inside a critical section
    /// can panic part-way through an update.
    fn lock(&self) -> MutexGuard<'_, u64> {
        self.shared
            .reserved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Bytes held in a [`MemoryBudget`], returned when this is dropped.
///
/// Growing, shrinking, merging and splitting change the budget's count by exactly the
/// bytes involved, so a handover between owners never passes through zero and never wakes
/// a [`wait_until_released`](MemoryBudget::wait_until_released) early.
#[derive(Debug)]
pub struct Reservation {
    budget: MemoryBudget,
    what: &'static str,
    bytes: u64,
}

impl Reservation {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Reserves `bytes` more. If they do not fit, the reservation and the budget stay as
    /// they were.
    pub fn try_grow(&mut self, bytes: u64) -> Result<(), BudgetExceeded> {
        self.budget.take(self.what, bytes)?;
        self.bytes += bytes;
        Ok(())
    }

    /// Returns `bytes` to the budget.
    ///
    /// # Panics
    ///
    /// If `bytes` is more than the reservation holds: that is an accounting error in the
    /// caller.
    pub fn shrink(&mut self, bytes: u64) {
        assert!(
            bytes <= self.bytes,
            "shrinking {} by {bytes} bytes, which is more than it holds ({})",
            self.what,
            self.bytes
        );
        self.bytes -= bytes;
        self.budget.give_back(bytes);
    }

    /// Takes over the bytes of `other`, which must come from the same budget.
    ///
    /// # Panics
    ///
    /// If `other` belongs to a different budget.
    pub fn merge(&mut self, mut other: Reservation) {
        assert!(
            Arc::ptr_eq(&self.budget.shared, &other.budget.shared),
            "merging reservations of different budgets ({} and {})",
            self.what,
            other.what
        );
        self.bytes += std::mem::take(&mut other.bytes);
    }

    /// Splits `bytes` off into a new reservation of the same budget, to hand to another
    /// owner.
    ///
    /// # Panics
    ///
    /// If `bytes` is more than the reservation holds.
    pub fn split(&mut self, bytes: u64) -> Reservation {
        assert!(
            bytes <= self.bytes,
            "splitting {bytes} bytes off {}, which holds only {}",
            self.what,
            self.bytes
        );
        self.bytes -= bytes;
        Reservation {
            budget: self.budget.clone(),
            what: self.what,
            bytes,
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.give_back(self.bytes);
    }
}

/// A reservation that does not fit in what the budget has left. Other modules wrap it in
/// the step that asked, such as admitting a query or staging a batch.
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(display(
    "memory budget exceeded for {what}: {needed} bytes needed, {available} available"
))]
#[snafu(visibility(pub(crate)))]
pub struct BudgetExceeded {
    pub what: &'static str,
    pub needed: u64,
    pub available: u64,
}

impl BudgetExceeded {
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Local
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Barrier,
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use super::*;

    /// How long a test waits for something that must happen. It bounds a failure; it is
    /// never what makes a passing test pass.
    const PATIENCE: Duration = Duration::from_secs(30);

    /// How long a test watches for something that must not happen.
    const GRACE: Duration = Duration::from_millis(100);

    #[test]
    fn a_reservation_returns_its_bytes_when_dropped() {
        let budget = MemoryBudget::new(100);
        let reservation = budget.try_reserve("head", 60).unwrap();
        assert_eq!(reservation.bytes(), 60);
        assert_eq!(budget.reserved(), 60);
        drop(reservation);
        assert_eq!(budget.reserved(), 0);
    }

    #[test]
    fn clones_share_one_counter() {
        let budget = MemoryBudget::new(100);
        let other = budget.clone();
        let _held = budget.try_reserve("a", 70).unwrap();
        assert_eq!(other.reserved(), 70);
        let error = other.try_reserve("b", 31).unwrap_err();
        assert_eq!(error.available, 30);
    }

    #[test]
    fn a_failed_reserve_changes_nothing_and_says_what_was_missing() {
        let budget = MemoryBudget::new(100);
        let held = budget.try_reserve("catalog", 60).unwrap();

        let error = budget.try_reserve("query", 41).unwrap_err();
        assert_eq!(
            error,
            BudgetExceeded {
                what: "query",
                needed: 41,
                available: 40
            }
        );
        assert_eq!(error.kind(), ErrorKind::Local);
        assert_eq!(
            error.to_string(),
            "memory budget exceeded for query: 41 bytes needed, 40 available"
        );
        assert_eq!(budget.reserved(), 60);

        // The refused bytes were not partly granted: exactly what is left still fits.
        let rest = budget.try_reserve("query", 40).unwrap();
        assert_eq!(budget.reserved(), 100);
        drop((held, rest));
        assert_eq!(budget.reserved(), 0);
    }

    #[test]
    fn nothing_beyond_the_limit_and_zero_always_fits() {
        let budget = MemoryBudget::new(10);
        assert!(budget.try_reserve("a", 11).is_err());
        assert!(budget.try_reserve("a", u64::MAX).is_err());
        let all = budget.try_reserve("a", 10).unwrap();
        let none = budget.try_reserve("b", 0).unwrap();
        assert_eq!(none.bytes(), 0);
        drop(none);
        assert_eq!(budget.reserved(), 10);
        drop(all);
        assert_eq!(MemoryBudget::new(0).try_reserve("a", 0).unwrap().bytes(), 0);
    }

    #[test]
    fn growing_past_the_limit_fails_and_keeps_the_reservation() {
        let budget = MemoryBudget::new(100);
        let mut reservation = budget.try_reserve("staging", 30).unwrap();
        reservation.try_grow(50).unwrap();
        assert_eq!((reservation.bytes(), budget.reserved()), (80, 80));

        let error = reservation.try_grow(21).unwrap_err();
        assert_eq!(
            error,
            BudgetExceeded {
                what: "staging",
                needed: 21,
                available: 20
            }
        );
        assert_eq!((reservation.bytes(), budget.reserved()), (80, 80));

        reservation.try_grow(20).unwrap();
        assert_eq!((reservation.bytes(), budget.reserved()), (100, 100));
    }

    #[test]
    fn shrinking_returns_part_of_a_reservation() {
        let budget = MemoryBudget::new(100);
        let mut reservation = budget.try_reserve("head", 80).unwrap();
        reservation.shrink(30);
        assert_eq!((reservation.bytes(), budget.reserved()), (50, 50));
        // The freed bytes can be reserved again.
        let other = budget.try_reserve("query", 50).unwrap();
        drop(other);
        reservation.shrink(50);
        assert_eq!((reservation.bytes(), budget.reserved()), (0, 0));
    }

    #[test]
    #[should_panic(expected = "more than it holds")]
    fn shrinking_by_more_than_held_is_an_accounting_error() {
        let budget = MemoryBudget::new(100);
        let mut reservation = budget.try_reserve("head", 10).unwrap();
        reservation.shrink(11);
    }

    #[test]
    fn merging_and_splitting_move_bytes_without_changing_the_count() {
        let budget = MemoryBudget::new(100);
        let mut first = budget.try_reserve("stage", 30).unwrap();
        let second = budget.try_reserve("stage", 20).unwrap();

        first.merge(second);
        assert_eq!((first.bytes(), budget.reserved()), (50, 50));

        let part = first.split(15);
        assert_eq!(
            (first.bytes(), part.bytes(), budget.reserved()),
            (35, 15, 50)
        );

        // The pieces are independent owners: dropping one returns only its bytes.
        drop(part);
        assert_eq!((first.bytes(), budget.reserved()), (35, 35));
        drop(first);
        assert_eq!(budget.reserved(), 0);
    }

    #[test]
    fn a_handover_never_passes_through_zero() {
        let budget = MemoryBudget::new(100);
        let owner = budget.try_reserve("head", 40).unwrap();

        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let waiter = thread::spawn({
            let budget = budget.clone();
            move || {
                started_tx.send(()).unwrap();
                budget.wait_until_released();
                done_tx.send(()).unwrap();
            }
        });
        started_rx.recv_timeout(PATIENCE).unwrap();

        // The only reservation changes hands twice. Releasing and re-reserving would pass
        // through zero and could wake the waiter.
        let mut new_owner = budget.try_reserve("head", 0).unwrap();
        new_owner.merge(owner);
        let part = new_owner.split(40);
        assert_eq!(
            (new_owner.bytes(), part.bytes(), budget.reserved()),
            (0, 40, 40)
        );
        drop(new_owner);
        assert!(
            done_rx.recv_timeout(GRACE).is_err(),
            "woke during a handover"
        );

        drop(part);
        done_rx.recv_timeout(PATIENCE).unwrap();
        waiter.join().unwrap();
    }

    #[test]
    #[should_panic(expected = "different budgets")]
    fn merging_reservations_of_different_budgets_is_a_programming_error() {
        let mut a = MemoryBudget::new(10).try_reserve("a", 1).unwrap();
        let b = MemoryBudget::new(10).try_reserve("b", 1).unwrap();
        a.merge(b);
    }

    #[test]
    #[should_panic(expected = "holds only")]
    fn splitting_off_more_than_held_is_an_accounting_error() {
        let budget = MemoryBudget::new(100);
        let mut reservation = budget.try_reserve("head", 10).unwrap();
        let _ = reservation.split(11);
    }

    #[test]
    fn concurrent_reservations_never_exceed_the_limit_and_end_at_zero() {
        const LIMIT: u64 = 300;
        const CHUNK: u64 = 100;
        const THREADS: usize = 8;
        const ROUNDS: usize = 2_000;

        let budget = MemoryBudget::new(LIMIT);
        // What the threads believe they hold: the budget must never have admitted more.
        let held = AtomicU64::new(0);
        let admitted = AtomicU64::new(0);
        let start = Barrier::new(THREADS);

        thread::scope(|scope| {
            for _ in 0..THREADS {
                scope.spawn(|| {
                    start.wait();
                    for _ in 0..ROUNDS {
                        let Ok(reservation) = budget.try_reserve("test", CHUNK) else {
                            continue;
                        };
                        let now = held.fetch_add(CHUNK, Ordering::SeqCst) + CHUNK;
                        assert!(now <= LIMIT, "{now} bytes held under a limit of {LIMIT}");
                        admitted.fetch_add(1, Ordering::SeqCst);
                        held.fetch_sub(CHUNK, Ordering::SeqCst);
                        drop(reservation);
                    }
                });
            }
        });

        assert!(admitted.load(Ordering::SeqCst) > 0);
        assert_eq!(budget.reserved(), 0);
        assert_eq!(held.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn concurrent_grow_and_shrink_end_at_zero() {
        const THREADS: usize = 4;
        const ROUNDS: usize = 2_000;

        const LIMIT: u64 = 100;

        let budget = MemoryBudget::new(LIMIT);
        let start = Barrier::new(THREADS);

        thread::scope(|scope| {
            for _ in 0..THREADS {
                scope.spawn(|| {
                    start.wait();
                    let mut reservation = budget.try_reserve("test", 0).unwrap();
                    for round in 0..ROUNDS {
                        // The threads settle near 40 bytes each, so some grows are refused.
                        let _ = reservation.try_grow(10 + (round % 7) as u64);
                        if round % 3 == 0 {
                            reservation.shrink(reservation.bytes() / 2);
                        }
                        assert!(budget.reserved() <= LIMIT);
                    }
                });
            }
        });

        assert_eq!(budget.reserved(), 0);
    }

    #[test]
    fn waiting_returns_at_once_when_nothing_is_reserved() {
        MemoryBudget::new(10).wait_until_released();
    }

    #[test]
    fn waiting_returns_when_another_thread_drops_the_last_reservation() {
        let budget = MemoryBudget::new(100);
        let first = budget.try_reserve("a", 10).unwrap();
        let second = budget.try_reserve("b", 20).unwrap();

        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let waiter = thread::spawn({
            let budget = budget.clone();
            move || {
                started_tx.send(()).unwrap();
                budget.wait_until_released();
                // Nothing may be held when the wait returns.
                done_tx.send(budget.reserved()).unwrap();
            }
        });
        started_rx.recv_timeout(PATIENCE).unwrap();

        // One reservation is still held, so the waiter keeps waiting.
        drop(first);
        assert!(
            done_rx.recv_timeout(GRACE).is_err(),
            "woke with bytes reserved"
        );

        drop(second);
        assert_eq!(done_rx.recv_timeout(PATIENCE), Ok(0));
        waiter.join().unwrap();
    }

    // A wakeup lost between the waiter's check and its sleep would leave it blocked for
    // good, so the last drop is raced against the start of the wait many times over. A
    // lost wakeup shows as a waiter that never reports. The waiter is not scoped: a scope
    // would wait for it to finish, and a test that failed would hang instead.
    #[test]
    fn a_wait_racing_the_last_drop_is_never_left_asleep() {
        for round in 0..500 {
            let budget = MemoryBudget::new(10);
            let reservation = budget.try_reserve("test", 10).unwrap();
            let race = Arc::new(Barrier::new(2));
            let (done_tx, done_rx) = mpsc::channel();

            let waiter = thread::spawn({
                let (budget, race) = (budget.clone(), race.clone());
                move || {
                    race.wait();
                    budget.wait_until_released();
                    done_tx.send(()).unwrap();
                }
            });
            race.wait();
            drop(reservation);
            done_rx
                .recv_timeout(PATIENCE)
                .unwrap_or_else(|_| panic!("round {round}: the waiter missed the wakeup"));
            waiter.join().unwrap();
        }
    }
}
