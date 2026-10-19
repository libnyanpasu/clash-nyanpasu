//! One recorder for port, allocation, publication and physical I/O boundaries.

use std::{
    fmt,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{Arc, Mutex},
};

use crate::storage::{Result, StorageError};

/// What the armed ordinal does, independently of the boundary's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Injection {
    /// Return the boundary's ordinary failure, latching storage when appropriate.
    Fault,
    /// Invalidate the instance before any unwinding or backend destruction.
    Crash,
}

/// A private unwind payload, never confused with a failed assertion in the operation.
#[derive(Debug)]
struct Interrupted;

/// Shared only by the explicitly constructed objects of one fixture; never process state.
#[derive(Clone, Default)]
pub struct FaultSchedule(Arc<Mutex<State>>);

/// Recording is disabled during fixture construction and oracle reads.
#[derive(Default)]
struct State {
    /// Only the operation under test contributes ordinals.
    enabled: bool,
    /// Disk replays also record retries and destructor I/O after their one injected failure.
    record_tail: bool,
    /// Boundary names in encounter order, including retries when tail recording is enabled.
    trace: Vec<&'static str>,
    /// One zero-based target, or a recording run.
    armed: Option<(usize, Injection)>,
    /// An armed replay cannot finish until this becomes true.
    fired: bool,
    /// Admission pressure persists for this attempt, including any fallback allocation.
    admission_failed: bool,
    /// Backend-specific latching, without owning its live handle.
    fault: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Invalidates the old generation before teardown can perform I/O.
    crash: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Weak observations of every attached root, including failed opens.
    budgets: Vec<Box<dyn Fn() -> u64 + Send + Sync>>,
    /// Optional barrier rendezvous at one recorded ordinal, removed before calling it.
    observe: Option<(usize, Box<dyn FnOnce() + Send>)>,
}

impl fmt::Debug for FaultSchedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaultSchedule").finish_non_exhaustive()
    }
}

impl FaultSchedule {
    /// Installs backend-specific invalidation, without owning a backend instance.
    pub(crate) fn actions(
        &self,
        fault: impl Fn() + Send + Sync + 'static,
        crash: impl Fn() + Send + Sync + 'static,
    ) {
        let mut state = self.0.lock().unwrap();
        state.fault = Some(Arc::new(fault));
        state.crash = Some(Arc::new(crash));
    }

    /// Starts one successful trace or one replay; ordinals start at zero on every instance.
    pub(crate) fn start(&self, armed: Option<(usize, Injection)>) {
        let mut state = self.0.lock().unwrap();
        state.enabled = true;
        state.record_tail = false;
        state.trace.clear();
        state.armed = armed;
        state.fired = false;
        state.admission_failed = false;
    }

    /// Records the whole replay, including later syncs after an explicitly absorbed disk error.
    #[cfg(feature = "redb")]
    pub(crate) fn start_with_tail(&self, armed: (usize, Injection)) {
        self.start(Some(armed));
        self.0.lock().unwrap().record_tail = true;
    }

    /// Stops before oracle work and refuses a replay that never reached its armed ordinal.
    pub(crate) fn finish(&self) -> Vec<&'static str> {
        let mut state = self.0.lock().unwrap();
        state.enabled = false;
        state.admission_failed = false;
        assert!(state.observe.is_none(), "unreached observation");
        assert!(
            state.armed.is_none() || state.fired,
            "unfired {:?}: {:?}",
            state.armed,
            state.trace
        );
        std::mem::take(&mut state.trace)
    }

    /// Parks at one successful ordinal without injecting failure; the caller supplies barriers.
    #[cfg(feature = "redb")]
    pub(crate) fn observe(&self, ordinal: usize, callback: impl FnOnce() + Send + 'static) {
        self.0.lock().unwrap().observe = Some((ordinal, Box::new(callback)));
    }

    /// Records a boundary and consumes its injection once. No callback holds this mutex.
    fn hit(&self, name: &'static str, storage: bool) -> Option<Injection> {
        let (action, observe) = {
            let mut state = self.0.lock().unwrap();
            if !state.enabled {
                return None;
            }
            let ordinal = state.trace.len();
            state.trace.push(name);
            let observe = if state.observe.as_ref().is_some_and(|(at, _)| *at == ordinal) {
                state.observe.take().map(|(_, callback)| callback)
            } else {
                None
            };
            let action = match state.armed {
                Some((at, injection)) if at == ordinal => {
                    state.fired = true;
                    state.enabled = state.record_tail;
                    let callback = match injection {
                        Injection::Fault if storage => state.fault.clone(),
                        Injection::Fault => None,
                        Injection::Crash => state.crash.clone(),
                    };
                    Some((injection, callback))
                }
                _ => None,
            };
            (action, observe)
        };
        if let Some(observe) = observe {
            observe();
        }
        action.map(|(injection, callback)| {
            if let Some(callback) = callback {
                callback();
            }
            injection
        })
    }

    /// A storage call fails normally; a process crash stops before any later instructions.
    pub(crate) fn step(&self, name: &'static str) -> Result<()> {
        match self.hit(name, true) {
            None => Ok(()),
            Some(Injection::Fault) => Err(StorageError::Failed {
                source: "scheduled fault".into(),
            }),
            Some(Injection::Crash) => resume_unwind(Box::new(Interrupted)),
        }
    }

    /// An infallible in-memory transition has no error return; stop after invalidating storage.
    pub(crate) fn boundary(&self, name: &'static str) {
        if self.step(name).is_err() {
            resume_unwind(Box::new(Interrupted));
        }
    }

    /// A nonzero admission can fail locally, without poisoning the backend.
    pub(crate) fn admission(&self, name: &'static str) -> bool {
        if self.0.lock().unwrap().admission_failed {
            return false;
        }
        match self.hit(name, false) {
            None => true,
            Some(Injection::Fault) => {
                self.0.lock().unwrap().admission_failed = true;
                false
            }
            Some(Injection::Crash) => resume_unwind(Box::new(Interrupted)),
        }
    }

    /// I/O reports both cases through redb, after crash invalidation has frozen the image.
    #[cfg(feature = "redb")]
    pub(crate) fn io(&self, name: &'static str) -> std::io::Result<()> {
        match self.hit(name, true) {
            None => Ok(()),
            Some(_) => Err(std::io::Error::other("scheduled I/O failure")),
        }
    }

    /// A weak observation cannot keep a reservation or a budget alive itself.
    pub(crate) fn budget(&self, reserved: impl Fn() -> u64 + Send + Sync + 'static) {
        self.0.lock().unwrap().budgets.push(Box::new(reserved));
    }

    /// All allocation envelopes have drained, including budgets replaced during recovery.
    pub(crate) fn assert_drained(&self) {
        for reserved in &self.0.lock().unwrap().budgets {
            assert_eq!(reserved(), 0, "scheduled operation leaked a reservation");
        }
    }
}

/// Catches only the schedule's private stop; a product panic or oracle assertion still fails.
pub(crate) fn interrupted<T>(run: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(value) => Some(value),
        Err(payload) if payload.is::<Interrupted>() => None,
        Err(payload) => resume_unwind(payload),
    }
}
