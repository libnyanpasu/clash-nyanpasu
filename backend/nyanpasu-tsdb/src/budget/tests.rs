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

mod charged {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use super::{super::account::Change, *};

    #[test]
    fn failed_growth_preserves_the_buffer_and_its_contents() {
        let budget = MemoryBudget::new(24);
        let account = MemoryAccount::new(&budget, "test", MemoryPlan::default()).unwrap();
        let mut buffer = account.buffer::<u64>(2).unwrap();
        buffer.extend_from_slice(&[11, 22]);
        let pointer = buffer.as_ptr();
        let capacity = buffer.capacity();
        let error = buffer.try_grow_to(4).unwrap_err();
        assert_eq!(error.available, 8);
        assert_eq!(
            (&*buffer, buffer.as_ptr(), buffer.capacity()),
            (&[11, 22][..], pointer, capacity)
        );
        assert_eq!(account.live.get(), 16);
        drop(buffer);
        drop(account);
        assert_eq!(budget.reserved(), 0);
    }

    #[test]
    fn replacement_records_old_and_new_peak_before_retirement() {
        let budget = MemoryBudget::new(48);
        let account = MemoryAccount::new(&budget, "test", MemoryPlan::default()).unwrap();
        let observations = Rc::new(RefCell::new(Vec::new()));
        let observed = observations.clone();
        *account.observer.borrow_mut() = Some(Box::new(move |event| {
            observed.borrow_mut().push(event);
        }));
        let mut buffer = account.buffer::<u64>(2).unwrap();
        buffer.extend_from_slice(&[11, 22]);
        let old = (buffer.capacity() * size_of::<u64>()) as u64;
        buffer.try_grow_to(4).unwrap();
        let new = (buffer.capacity() * size_of::<u64>()) as u64;
        assert_eq!(&*buffer, &[11, 22]);
        let mut actual_live = 0;
        let mut actual_peak = 0;
        let events = observations.borrow();
        for event in events.iter() {
            match event.kind {
                Change::Allocate => actual_live += event.bytes,
                Change::Retire => actual_live -= event.bytes,
                Change::Transfer => unreachable!(),
            }
            actual_peak = actual_peak.max(actual_live);
            assert_eq!(event.live, actual_live);
            assert_eq!(event.peak, actual_peak);
        }
        assert_eq!(actual_peak, old + new);
        assert_eq!(actual_live, new);
        drop(events);
        assert_eq!(account.allocated.get(), old + new);
        buffer.clear();
        assert_eq!(account.live.get(), new);
        assert_eq!(buffer.capacity() as u64 * size_of::<u64>() as u64, new);
    }

    struct DropProbe {
        budget: MemoryBudget,
        needed: u64,
        dropped: Rc<Cell<usize>>,
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            assert!(self.budget.reserved() >= self.needed);
            self.dropped.set(self.dropped.get() + 1);
        }
    }

    #[test]
    fn payload_drops_before_its_charge_on_success_error_and_unwind() {
        for mode in 0..3 {
            let budget = MemoryBudget::new(1024);
            let dropped = Rc::new(Cell::new(0));
            let account = MemoryAccount::new(&budget, "test", MemoryPlan::default()).unwrap();
            let retired = dropped.clone();
            *account.observer.borrow_mut() = Some(Box::new(move |event| {
                if event.kind == Change::Retire && event.bytes != 0 {
                    assert_eq!(retired.get(), 1);
                }
            }));
            let mut buffer = account.buffer(1).unwrap();
            buffer.push(DropProbe {
                budget: budget.clone(),
                needed: size_of::<DropProbe>() as u64,
                dropped: dropped.clone(),
            });
            match mode {
                0 => {
                    let retained = account.finish(buffer).unwrap();
                    drop(account);
                    assert_eq!(dropped.get(), 0);
                    drop(retained);
                }
                1 => {
                    assert!(buffer.try_grow_to(1024).is_err());
                    drop(buffer);
                    drop(account);
                }
                _ => {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _retained = account.finish(buffer).unwrap();
                        panic!("probe unwind");
                    }));
                    assert!(outcome.is_err());
                    drop(account);
                }
            }
            assert_eq!(dropped.get(), 1);
            assert_eq!(budget.reserved(), 0);
        }
    }

    #[test]
    fn zero_sized_buffers_are_free_and_layout_overflow_is_refused() {
        let budget = MemoryBudget::new(0);
        let account = MemoryAccount::new(&budget, "test", MemoryPlan::default()).unwrap();
        let mut buffer = account.buffer::<()>(usize::MAX).unwrap();
        buffer.push(());
        buffer.try_grow_to(usize::MAX).unwrap();
        assert_eq!(budget.reserved(), 0);
        assert_eq!(account.live.get(), 0);
        assert!(account.buffer::<u64>(usize::MAX).is_err());
        assert!(MemoryPlan::array::<u8>(usize::MAX).is_err());
        assert!(MemoryPlan(u64::MAX).and(MemoryPlan(1)).is_err());
        assert_eq!(budget.reserved(), 0);
    }

    #[test]
    #[should_panic]
    fn bounded_appends_cannot_grow_storage() {
        let budget = MemoryBudget::new(1);
        let account = MemoryAccount::new(&budget, "test", MemoryPlan::default()).unwrap();
        let mut buffer = account.buffer::<u8>(1).unwrap();
        buffer.push(1);
        buffer.extend_from_slice(&[2]);
    }

    #[test]
    fn every_admission_boundary_can_fail_without_losing_storage() {
        for boundary in 0..3 {
            let budget = MemoryBudget::new(1024);
            let account = MemoryAccount::new(&budget, "test", MemoryPlan::default()).unwrap();
            let mut buffer = account.buffer::<u64>(1).unwrap();
            buffer.push(7);
            account.fail_after(boundary);
            for capacity in [2, 4, 8] {
                let old_pointer = buffer.as_ptr();
                let old_capacity = buffer.capacity();
                if buffer.try_grow_to(capacity).is_err() {
                    assert_eq!(buffer.as_ptr(), old_pointer);
                    assert_eq!(buffer.capacity(), old_capacity);
                    break;
                }
            }
            assert_eq!(&*buffer, &[7]);
            drop(buffer);
            assert_eq!(account.live.get(), 0);
            drop(account);
            assert_eq!(budget.reserved(), 0);
        }
    }

    #[test]
    fn retaining_moves_storage_and_credit_without_releasing_it() {
        let budget = MemoryBudget::new(32);
        let account = MemoryAccount::new(&budget, "test", MemoryPlan(32)).unwrap();
        let mut buffer = account.buffer::<u64>(2).unwrap();
        buffer.extend_from_slice(&[1, 2]);
        let pointer = buffer.as_ptr();
        let retained = account.finish(buffer).unwrap();
        assert_eq!(retained.get().as_ptr(), pointer);
        assert_eq!(budget.reserved(), 32);
        drop(account);
        assert_eq!(budget.reserved(), 16);
        assert_eq!(&**retained.get(), &[1, 2]);
        drop(retained);
        assert_eq!(budget.reserved(), 0);
    }

    struct FailingBuffer<'a>(BudgetVec<'a, DropProbe>);

    impl Retain<DropProbe> for FailingBuffer<'_> {
        fn retain(
            self,
            retained: &mut Retained<'_>,
        ) -> Result<FrozenVec<DropProbe>, BudgetExceeded> {
            let _payload = self.0.retain(retained)?;
            Err(BudgetExceeded {
                what: "test finish",
                needed: 1,
                available: 0,
            })
        }
    }

    #[test]
    fn a_failed_finish_drops_partial_payload_before_transferred_credit() {
        let budget = MemoryBudget::new(1024);
        let account = MemoryAccount::new(&budget, "test", MemoryPlan::default()).unwrap();
        let dropped = Rc::new(Cell::new(0));
        let mut buffer = account.buffer(1).unwrap();
        buffer.push(DropProbe {
            budget: budget.clone(),
            needed: size_of::<DropProbe>() as u64,
            dropped: dropped.clone(),
        });
        assert!(account.finish(FailingBuffer(buffer)).is_err());
        assert_eq!(dropped.get(), 1);
        assert_eq!(account.live.get(), 0);
        drop(account);
        assert_eq!(budget.reserved(), 0);
    }
}
