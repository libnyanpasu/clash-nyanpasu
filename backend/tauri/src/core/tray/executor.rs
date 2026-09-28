//! Tray work as data, run by one drain on the main thread.
//!
//! Every tray build and repaint is requested through [`request`]: requests
//! merge into the queue, and at most one drain is scheduled or running at a
//! time. Requests made during a drain only merge work, so tray work never
//! nests and never interleaves, whatever thread asked for it. A request made
//! on an idle main thread may run the drain inline.

use std::ops::{BitOr, ControlFlow};

use parking_lot::Mutex;

use super::display::Paint;

/// Tray work that was asked for and has not run yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrayWork {
    rebuild: bool,
    part: bool,
    proxies: bool,
}

impl TrayWork {
    /// Build the menu from the stored view and publish it.
    pub const REBUILD: Self = Self {
        rebuild: true,
        part: false,
        proxies: false,
    };
    /// Repaint the checkmarks, icon and tooltip from the stored view.
    pub const PART: Self = Self {
        rebuild: false,
        part: true,
        proxies: false,
    };
    /// Bring the proxy selection up to the latest proxy snapshot.
    pub const PROXIES: Self = Self {
        rebuild: false,
        part: false,
        proxies: true,
    };

    fn is_empty(self) -> bool {
        !(self.rebuild || self.part || self.proxies)
    }
}

impl BitOr for TrayWork {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self {
            rebuild: self.rebuild || other.rebuild,
            part: self.part || other.part,
            proxies: self.proxies || other.proxies,
        }
    }
}

/// The work waiting for the drain.
#[derive(Debug, Default)]
pub(super) struct TrayQueue {
    pending: TrayWork,
    /// A drain is scheduled or running. Only the drain clears it, once it
    /// finds nothing left, so a request made meanwhile only merges.
    scheduled: bool,
}

impl TrayQueue {
    /// Merges `work`; true when the caller has to schedule the drain.
    fn push(&mut self, work: TrayWork) -> bool {
        self.pending = self.pending | work;
        !std::mem::replace(&mut self.scheduled, true)
    }

    /// The pending work, or `None` once there is none and the drain ends.
    fn take(&mut self) -> Option<TrayWork> {
        let work = std::mem::take(&mut self.pending);
        if work.is_empty() {
            self.scheduled = false;
            return None;
        }
        Some(work)
    }
}

/// One step of tray work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Rebuild,
    Part,
    Proxies,
}

/// The tray, as the drain runs it.
pub(super) trait TrayTarget {
    type Menu;
    /// What a repaint may paint, and whether the tray icon exists.
    fn observe(&self) -> (Paint<Self::Menu>, bool);
    /// Runs `step`. `Break` skips the rest of the work it belongs to: the
    /// step failed and recorded it, or it requested a rebuild that repeats
    /// those steps after it.
    fn run(&mut self, step: Step) -> ControlFlow<()>;
}

/// Whether the tray has to be rebuilt before anything is repainted: its menu
/// is unknown, or a menu was built and its icon is gone.
pub(super) fn needs_rebuild<M>(paint: &Paint<M>, icon: bool) -> bool {
    match paint {
        Paint::Unknown => true,
        Paint::Menu(_) => !icon,
        Paint::NotBuilt => false,
    }
}

/// The steps that carry out `work` on a tray in this state, in order.
pub(super) fn steps<M>(work: TrayWork, paint: &Paint<M>, icon: bool) -> Vec<Step> {
    if work.is_empty() {
        return Vec::new();
    }
    // A rebuilt menu shows placeholder checkmarks and the proxy selection it
    // read, so a checkmark repaint and a proxy reconcile against the new
    // record follow every rebuild.
    if work.rebuild || needs_rebuild(paint, icon) {
        return vec![Step::Rebuild, Step::Part, Step::Proxies];
    }
    // Nothing is built yet: the first build renders the latest view.
    if matches!(paint, Paint::NotBuilt) {
        return Vec::new();
    }
    let mut steps = Vec::new();
    if work.part {
        steps.push(Step::Part);
    }
    if work.proxies {
        steps.push(Step::Proxies);
    }
    steps
}

/// Merges `work` into the queue and, unless a drain is already scheduled or
/// running, schedules one with `schedule`. Requests made during a drain only
/// merge work, which the running drain takes next. On an idle main thread
/// `schedule` may run the drain inline, before this returns.
pub(super) fn request(
    queue: &Mutex<TrayQueue>,
    work: TrayWork,
    schedule: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let must_schedule = queue.lock().push(work);
    if !must_schedule {
        return Ok(());
    }
    schedule().inspect_err(|_| {
        // Nothing will drain the work, so the next request schedules again.
        queue.lock().scheduled = false;
    })
}

/// Runs the queued work until none is left. The one place tray work runs,
/// on the main thread.
pub(super) fn drain(queue: &Mutex<TrayQueue>, tray: &mut impl TrayTarget) {
    loop {
        let taken = queue.lock().take();
        let Some(work) = taken else {
            return;
        };
        let plan = {
            let (paint, icon) = tray.observe();
            steps(work, &paint, icon)
        };
        for step in plan {
            if tray.run(step).is_break() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{
        super::display::{ProxySection, Publication, Shown, TrayDisplay},
        *,
    };

    const FULL: &[Step] = &[Step::Rebuild, Step::Part, Step::Proxies];
    const NONE: &[Step] = &[];

    /// Counts the drains scheduled. The test runs each one itself, as the
    /// main thread would.
    fn counting(schedules: &Cell<usize>) -> impl FnOnce() -> anyhow::Result<()> + '_ {
        move || {
            schedules.set(schedules.get() + 1);
            Ok(())
        }
    }

    /// A tray whose steps record themselves against a real display record,
    /// deciding as the live steps do.
    struct FakeTray<'q> {
        queue: &'q Mutex<TrayQueue>,
        display: TrayDisplay<&'static str>,
        icon: bool,
        /// Every step run, in order.
        ran: Vec<Step>,
        /// Requested from inside the first step of this kind that runs.
        reentrant: Option<(Step, TrayWork)>,
        /// Marks the record unknown right after the next rebuild publishes,
        /// as a checkmark failure landing there would.
        invalidate_after_publish: bool,
        /// Rebuilds remove the icon and fail to build its replacement.
        rebuild_fails: bool,
    }

    impl<'q> FakeTray<'q> {
        /// A tray showing a published menu.
        fn known(queue: &'q Mutex<TrayQueue>) -> Self {
            let mut display = TrayDisplay::new();
            display.published(Publication::Complete {
                attached: "old",
                section: ProxySection::default(),
            });
            Self {
                queue,
                display,
                icon: true,
                ran: Vec::new(),
                reentrant: None,
                invalidate_after_publish: false,
                rebuild_fails: false,
            }
        }

        /// A request from inside a step. A drain is running, so scheduling
        /// another would nest it: Tauri runs a task scheduled from the main
        /// thread at once.
        fn request(&self, work: TrayWork) {
            request(self.queue, work, || {
                panic!("a request from inside a step scheduled a nested drain")
            })
            .unwrap();
        }
    }

    impl TrayTarget for FakeTray<'_> {
        type Menu = &'static str;

        fn observe(&self) -> (Paint<&'static str>, bool) {
            (self.display.paint(), self.icon)
        }

        fn run(&mut self, step: Step) -> ControlFlow<()> {
            self.ran.push(step);
            // A drain that keeps requesting itself fails here instead of
            // spinning the test forever.
            assert!(
                self.ran.len() < 32,
                "the drain never settled: {:?}",
                self.ran
            );
            if let Some((during, work)) = self.reentrant
                && during == step
            {
                self.reentrant = None;
                self.request(work);
                self.request(work);
            }
            match step {
                Step::Rebuild if self.rebuild_fails => {
                    self.icon = false;
                    self.display
                        .published(Publication::Interrupted { attached: None });
                    ControlFlow::Break(())
                }
                Step::Rebuild => {
                    self.icon = true;
                    self.display.published(Publication::Complete {
                        attached: "new",
                        section: ProxySection::default(),
                    });
                    if std::mem::take(&mut self.invalidate_after_publish) {
                        self.display.repainted(None, Shown::Partly);
                    }
                    ControlFlow::Continue(())
                }
                Step::Part => {
                    let paint = self.display.paint();
                    if needs_rebuild(&paint, self.icon) {
                        self.request(TrayWork::REBUILD);
                        return ControlFlow::Break(());
                    }
                    if matches!(paint, Paint::NotBuilt) {
                        return ControlFlow::Break(());
                    }
                    self.display.repainted(None, Shown::Whole);
                    ControlFlow::Continue(())
                }
                Step::Proxies => {
                    if matches!(self.display.paint(), Paint::Unknown) {
                        self.request(TrayWork::REBUILD);
                        return ControlFlow::Break(());
                    }
                    ControlFlow::Continue(())
                }
            }
        }
    }

    #[test]
    fn requests_before_the_drain_merge_into_one_run() {
        let queue = Mutex::new(TrayQueue::default());
        let schedules = Cell::new(0);
        for work in [TrayWork::PROXIES, TrayWork::PART, TrayWork::PROXIES] {
            request(&queue, work, counting(&schedules)).unwrap();
        }
        let mut tray = FakeTray::known(&queue);
        drain(&queue, &mut tray);
        assert_eq!(tray.ran, [Step::Part, Step::Proxies]);
        assert_eq!(schedules.get(), 1);
    }

    #[test]
    fn a_request_while_a_drain_is_scheduled_does_not_schedule_another() {
        let queue = Mutex::new(TrayQueue::default());
        let schedules = Cell::new(0);
        request(&queue, TrayWork::PART, counting(&schedules)).unwrap();
        request(&queue, TrayWork::REBUILD, counting(&schedules)).unwrap();
        assert_eq!(schedules.get(), 1);

        let mut tray = FakeTray::known(&queue);
        drain(&queue, &mut tray);
        assert_eq!(tray.ran, FULL);
        assert!(!queue.lock().scheduled);

        // The drain ended, so the next request schedules a new one.
        request(&queue, TrayWork::PART, counting(&schedules)).unwrap();
        assert_eq!(schedules.get(), 2);
    }

    /// The step that asked is never interrupted: the work it asked for runs
    /// once, after it, in the same drain.
    #[test]
    fn a_request_from_inside_a_step_runs_once_after_that_step() {
        let queue = Mutex::new(TrayQueue::default());
        let schedules = Cell::new(0);
        request(&queue, TrayWork::PART, counting(&schedules)).unwrap();
        let mut tray = FakeTray::known(&queue);
        tray.reentrant = Some((Step::Part, TrayWork::PROXIES));
        drain(&queue, &mut tray);
        assert_eq!(tray.ran, [Step::Part, Step::Proxies]);
        assert_eq!(schedules.get(), 1);
        assert!(!queue.lock().scheduled);
    }

    /// Codex round 4: the record turns unknown between a rebuild's
    /// publication and its trailing checkmark repaint. That repaint requests
    /// exactly one rebuild, which runs next instead of re-entering the one
    /// that was running.
    #[test]
    fn an_invalidation_after_publication_queues_one_rebuild_that_runs_next() {
        let queue = Mutex::new(TrayQueue::default());
        let schedules = Cell::new(0);
        request(&queue, TrayWork::REBUILD, counting(&schedules)).unwrap();
        let mut tray = FakeTray::known(&queue);
        tray.invalidate_after_publish = true;
        drain(&queue, &mut tray);
        assert_eq!(
            tray.ran,
            [
                Step::Rebuild,
                Step::Part,
                Step::Rebuild,
                Step::Part,
                Step::Proxies
            ]
        );
        assert_eq!(schedules.get(), 1);
        assert!(matches!(tray.display.paint(), Paint::Menu("new")));
    }

    /// A failed rebuild leaves the rest of its work to the next request, so
    /// a rebuild that keeps failing cannot spin the main thread.
    #[test]
    fn a_failed_step_skips_the_rest_of_its_work() {
        let queue = Mutex::new(TrayQueue::default());
        let schedules = Cell::new(0);
        request(&queue, TrayWork::REBUILD, counting(&schedules)).unwrap();
        let mut tray = FakeTray::known(&queue);
        tray.rebuild_fails = true;
        drain(&queue, &mut tray);
        assert_eq!(tray.ran, [Step::Rebuild]);
        assert!(!queue.lock().scheduled);

        request(&queue, TrayWork::PART, counting(&schedules)).unwrap();
        drain(&queue, &mut tray);
        assert_eq!(tray.ran, [Step::Rebuild, Step::Rebuild]);
        assert_eq!(schedules.get(), 2);
    }

    #[test]
    fn a_failed_schedule_lets_the_next_request_schedule() {
        let queue = Mutex::new(TrayQueue::default());
        let failed = request(&queue, TrayWork::PART, || {
            anyhow::bail!("the event loop is gone")
        });
        assert!(failed.is_err());
        assert!(!queue.lock().scheduled);

        let schedules = Cell::new(0);
        request(&queue, TrayWork::PROXIES, counting(&schedules)).unwrap();
        assert_eq!(schedules.get(), 1);
        let mut tray = FakeTray::known(&queue);
        drain(&queue, &mut tray);
        assert_eq!(tray.ran, [Step::Part, Step::Proxies]);
    }

    /// A main thread whose scheduler runs the drain inline, as
    /// `run_on_main_thread` does when it is called there while idle.
    struct InlineMainThread<'q> {
        queue: &'q Mutex<TrayQueue>,
        schedules: Cell<usize>,
        ran: std::cell::RefCell<Vec<Step>>,
        /// A step is running; a nested one would find it set.
        in_step: Cell<bool>,
        /// Requested from inside the first step of this kind that runs.
        reentrant: Cell<Option<(Step, TrayWork)>>,
    }

    impl InlineMainThread<'_> {
        fn request(&self, work: TrayWork) {
            request(self.queue, work, || {
                self.schedules.set(self.schedules.get() + 1);
                drain(self.queue, &mut InlineTray(self));
                Ok(())
            })
            .unwrap();
        }
    }

    struct InlineTray<'m, 'q>(&'m InlineMainThread<'q>);

    impl TrayTarget for InlineTray<'_, '_> {
        type Menu = ();

        fn observe(&self) -> (Paint<()>, bool) {
            (Paint::Menu(()), true)
        }

        fn run(&mut self, step: Step) -> ControlFlow<()> {
            let main = self.0;
            assert!(!main.in_step.replace(true), "{step:?} ran inside a step");
            main.ran.borrow_mut().push(step);
            if let Some((during, work)) = main.reentrant.get()
                && during == step
            {
                main.reentrant.set(None);
                main.request(work);
                main.request(work);
            }
            main.in_step.set(false);
            ControlFlow::Continue(())
        }
    }

    /// An idle main-thread request runs the drain inline, once; requests made
    /// during that drain only merge and run after the step that made them.
    #[test]
    fn an_inline_drain_runs_once_and_requests_during_it_only_merge() {
        let queue = Mutex::new(TrayQueue::default());
        let main = InlineMainThread {
            queue: &queue,
            schedules: Cell::new(0),
            ran: Default::default(),
            in_step: Cell::new(false),
            reentrant: Cell::new(Some((Step::Part, TrayWork::PROXIES))),
        };
        main.request(TrayWork::PART);
        assert_eq!(*main.ran.borrow(), [Step::Part, Step::Proxies]);
        assert_eq!(main.schedules.get(), 1);
        assert!(!queue.lock().scheduled);

        main.request(TrayWork::PART);
        assert_eq!(*main.ran.borrow(), [Step::Part, Step::Proxies, Step::Part]);
        assert_eq!(main.schedules.get(), 2);
    }

    #[test]
    fn steps_follow_the_work_and_the_tray_state() {
        let works = [
            TrayWork::REBUILD,
            TrayWork::PART,
            TrayWork::PROXIES,
            TrayWork::PART | TrayWork::PROXIES,
        ];
        let part_and_proxies: &[Step] = &[Step::Part, Step::Proxies];
        /// A tray state and whether the icon exists, with the steps each of
        /// `works` takes on it.
        type Case = (&'static str, Paint<()>, bool, [&'static [Step]; 4]);
        let cases: [Case; 6] = [
            (
                "not built",
                Paint::NotBuilt,
                false,
                [FULL, NONE, NONE, NONE],
            ),
            (
                "not built, icon",
                Paint::NotBuilt,
                true,
                [FULL, NONE, NONE, NONE],
            ),
            (
                "known",
                Paint::Menu(()),
                true,
                [FULL, &[Step::Part], &[Step::Proxies], part_and_proxies],
            ),
            ("known, icon gone", Paint::Menu(()), false, [FULL; 4]),
            ("unknown", Paint::Unknown, true, [FULL; 4]),
            ("unknown, icon gone", Paint::Unknown, false, [FULL; 4]),
        ];
        for (label, paint, icon, expected) in &cases {
            for (work, expected) in works.iter().zip(expected) {
                assert_eq!(steps(*work, paint, *icon), *expected, "{work:?}, {label}");
            }
            let everything = TrayWork::REBUILD | TrayWork::PART | TrayWork::PROXIES;
            assert_eq!(steps(everything, paint, *icon), FULL, "{label}");
            assert_eq!(steps(TrayWork::default(), paint, *icon), NONE, "{label}");
        }
    }

    /// Codex round 4 minor: a menu mode change removed the old icon and its
    /// replacement failed to build. A later repaint of any kind rebuilds
    /// rather than skipping the missing icon.
    #[test]
    fn a_failed_icon_replacement_is_rebuilt_by_the_next_repaint() {
        let mut display = TrayDisplay::new();
        display.published(Publication::Complete {
            attached: "old",
            section: ProxySection::default(),
        });
        display.published(Publication::Interrupted { attached: None });
        for work in [TrayWork::PART, TrayWork::PROXIES] {
            assert_eq!(steps(work, &display.paint(), false), FULL, "{work:?}");
        }
    }
}
