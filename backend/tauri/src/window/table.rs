//! The windows this app created and the facts about each. Pure bookkeeping:
//! it names no Tauri type, so its rules hold without a window.
//!
//! Events only set facts. What the native window should look like follows
//! from the facts alone, so the result does not depend on the order the
//! events arrive in.

use std::collections::HashMap;
use tokio::task::AbortHandle;

#[derive(Debug)]
struct Entry<H> {
    base_label: String,
    /// The native window exists, built hidden.
    built: bool,
    /// The webview reported ready, or its fallback ran out.
    ready: bool,
    /// The window should be shown. Every open sets it, and a close clears it.
    wanted: bool,
    /// The webview's first ready is not yet acted on. It needs the built window.
    setup_pending: bool,
    /// The fallback that marks a window ready when its webview never reports.
    watchdog: Option<AbortHandle>,
    /// What the kind of window does when it is applied.
    hooks: H,
}

impl<H> Entry<H> {
    fn stop_watchdog(&mut self) {
        if let Some(watchdog) = self.watchdog.take() {
            watchdog.abort();
        }
    }
}

/// `H` is what a window kind contributes to applying its facts, which the
/// table keeps and hands back without looking at it.
#[derive(Debug)]
pub struct WindowTable<H = ()> {
    entries: HashMap<String, Entry<H>>,
}

impl<H> Default for WindowTable<H> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// Build a new window under this label.
    Build(String),
    /// The window exists; it is asked to be shown.
    Existing(String),
}

/// The facts about a built window at one moment. The one-shot work they carry
/// is taken out of the table.
#[derive(Debug)]
pub struct Facts<H> {
    pub ready: bool,
    pub wanted: bool,
    /// Set the webview up: its first ready is not yet acted on.
    pub setup: bool,
    pub hooks: H,
}

/// What to do to the native window so that it is visible exactly when it is
/// ready and wanted, and gone, or hidden, when it is not wanted.
#[derive(Debug, PartialEq, Eq)]
pub struct Actions {
    pub first_ready: bool,
    pub show: bool,
    /// The window is not wanted: it goes away, or is hidden if it is presented.
    pub retire: bool,
    /// The window is presented and not wanted: the kind is told before it
    /// goes.
    pub dismiss: bool,
}

impl<H> Facts<H> {
    /// `presented` is read from the native window when the facts are applied:
    /// it is visible, or minimized, which some platforms do not count as
    /// visible.
    pub fn actions(&self, presented: bool) -> Actions {
        Actions {
            first_ready: self.setup,
            show: self.ready && self.wanted,
            retire: !self.wanted,
            dismiss: !self.wanted && presented,
        }
    }
}

impl<H: Clone> WindowTable<H> {
    /// Decides what opening a window of `base_label` does. A window to build
    /// is registered before this returns, so its ready always comes after its
    /// registration.
    ///
    /// A singleton uses the base label. Other instances take `base-N` with the
    /// lowest free number.
    pub fn open(&mut self, base_label: &str, singleton: bool, hooks: H) -> Opened {
        let instances = self.instances(base_label);

        if singleton && let Some(label) = instances.first() {
            self.entries.get_mut(label).expect("listed").wanted = true;
            return Opened::Existing(label.clone());
        }

        let label = if instances.is_empty() {
            base_label.to_string()
        } else {
            (1..)
                .map(|number| format!("{base_label}-{number}"))
                .find(|label| !instances.contains(label))
                .expect("an unbounded range has a free label")
        };
        self.entries.insert(
            label.clone(),
            Entry {
                base_label: base_label.to_string(),
                built: false,
                ready: false,
                wanted: true,
                setup_pending: false,
                watchdog: None,
                hooks,
            },
        );
        Opened::Build(label)
    }

    /// The window is built and set up. Returns `None` for a window the table
    /// does not know, and otherwise whether it still waits for its webview to
    /// report ready, which is when it needs a fallback.
    pub fn built(&mut self, label: &str) -> Option<bool> {
        let entry = self.entries.get_mut(label)?;
        entry.built = true;
        Some(!entry.ready)
    }

    /// Records that the window's webview rendered. Returns `None` for a window
    /// the table does not know, and otherwise whether this was the first report.
    pub fn ready(&mut self, label: &str) -> Option<bool> {
        let entry = self.entries.get_mut(label)?;
        if entry.ready {
            return Some(false);
        }
        entry.ready = true;
        entry.setup_pending = true;
        entry.stop_watchdog();
        Some(true)
    }

    /// The window is no longer wanted: a close was asked for, by the app or
    /// by the user. Returns whether that changed anything.
    pub fn unwant(&mut self, label: &str) -> bool {
        match self.entries.get_mut(label) {
            Some(entry) if entry.wanted => {
                entry.wanted = false;
                true
            }
            _ => false,
        }
    }

    /// Whether the window is wanted: opened and not closed since.
    pub fn wanted(&self, label: &str) -> bool {
        self.entries.get(label).is_some_and(|entry| entry.wanted)
    }

    /// The facts about a built window now, or `None` while it is not built
    /// or unknown: there is nothing to act on, and what is pending waits.
    pub fn facts(&mut self, label: &str) -> Option<Facts<H>> {
        let entry = self.entries.get_mut(label)?;
        if !entry.built {
            return None;
        }
        Some(Facts {
            ready: entry.ready,
            wanted: entry.wanted,
            setup: std::mem::take(&mut entry.setup_pending),
            hooks: entry.hooks.clone(),
        })
    }

    /// Hands a window that waits for ready its fallback, and returns whether
    /// it was kept. A window that is gone, or already ready, needs none; the
    /// caller stops the task then.
    #[must_use]
    pub fn set_watchdog(&mut self, label: &str, watchdog: &AbortHandle) -> bool {
        match self.entries.get_mut(label) {
            Some(entry) if !entry.ready => {
                entry.stop_watchdog();
                entry.watchdog = Some(watchdog.clone());
                true
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, label: &str) {
        if let Some(mut entry) = self.entries.remove(label) {
            entry.stop_watchdog();
        }
    }

    /// The labels of the windows opened under `base_label`, in label order.
    pub fn instances(&self, base_label: &str) -> Vec<String> {
        let mut labels: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.base_label == base_label)
            .map(|(label, _)| label.clone())
            .collect();
        labels.sort();
        labels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built_window(table: &mut WindowTable, base_label: &str, singleton: bool) -> String {
        let Opened::Build(label) = table.open(base_label, singleton, ()) else {
            panic!("expected a window to build");
        };
        table.built(&label);
        label
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Event {
        Built,
        Ready,
        Open,
        Close,
        Minimize,
    }

    fn permutations(events: &[Event]) -> Vec<Vec<Event>> {
        if events.len() <= 1 {
            return vec![events.to_vec()];
        }
        let mut all = Vec::new();
        for index in 0..events.len() {
            let mut rest = events.to_vec();
            let first = rest.remove(index);
            for mut tail in permutations(&rest) {
                tail.insert(0, first);
                all.push(tail);
            }
        }
        all
    }

    /// The native window as the model tracks it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Native {
        Absent,
        Hidden,
        Shown,
        Minimized,
    }

    impl Native {
        fn presented(self) -> bool {
            matches!(self, Self::Shown | Self::Minimized)
        }
    }

    /// The table, a fake native window, and the applies the manager has queued
    /// but not run yet. A retired window is hidden, so it can come back.
    struct Model {
        table: WindowTable,
        label: String,
        native: Native,
        queued: u32,
        dismissed: u32,
        presented_retired: u32,
        setups: u32,
    }

    impl Model {
        fn new() -> Self {
            let mut table = WindowTable::default();
            let Opened::Build(label) = table.open("main", true, ()) else {
                panic!("expected a window to build");
            };
            Self {
                table,
                label,
                native: Native::Absent,
                queued: 0,
                dismissed: 0,
                presented_retired: 0,
                setups: 0,
            }
        }

        /// What the manager does for each event: set the facts, and queue an
        /// apply when one changed, or on every open.
        fn event(&mut self, event: Event) {
            match event {
                Event::Built => {
                    if self.table.built(&self.label).is_some() {
                        self.native = Native::Hidden;
                        self.queued += 1;
                    }
                }
                Event::Ready => {
                    if self.table.ready(&self.label) == Some(true) {
                        self.queued += 1;
                    }
                }
                Event::Open => {
                    self.table.open("main", true, ());
                    self.queued += 1;
                }
                Event::Close => {
                    if self.table.unwant(&self.label) {
                        self.queued += 1;
                    }
                }
                Event::Minimize => {
                    if self.native == Native::Shown {
                        self.native = Native::Minimized;
                    }
                }
            }
        }

        /// Runs one queued apply, if there is one.
        fn drain_one(&mut self) {
            if self.queued == 0 {
                return;
            }
            self.queued -= 1;
            let Some(facts) = self.table.facts(&self.label) else {
                return;
            };
            let presented = self.native.presented();
            let actions = facts.actions(presented);
            self.setups += u32::from(actions.first_ready);
            if actions.show {
                self.native = Native::Shown;
            } else if actions.retire {
                self.presented_retired += u32::from(presented);
                self.dismissed += u32::from(actions.dismiss);
                if presented {
                    self.native = Native::Hidden;
                }
            }
        }
    }

    #[test]
    fn the_native_window_follows_the_facts_whatever_the_order_of_events_and_applies() {
        // A watchdog firing is a second ready, and a close by the user, or by
        // a tray menu losing focus, is a second close.
        let events = [
            Event::Built,
            Event::Ready,
            Event::Ready,
            Event::Open,
            Event::Close,
            Event::Close,
            Event::Minimize,
        ];
        let orders: std::collections::BTreeSet<Vec<Event>> =
            permutations(&events).into_iter().collect();
        for order in &orders {
            // Which events have one queued apply run right after them.
            for drains in 0..(1u32 << order.len()) {
                let mut model = Model::new();
                let (mut built, mut ready, mut wanted) = (false, false, true);
                for (position, event) in order.iter().enumerate() {
                    model.event(*event);
                    match event {
                        Event::Built => built = true,
                        Event::Ready => ready = true,
                        Event::Open => wanted = true,
                        Event::Close => wanted = false,
                        Event::Minimize => {}
                    }
                    if drains >> position & 1 == 1 {
                        model.drain_one();
                    }
                }
                while model.queued > 0 {
                    model.drain_one();
                }

                let case = format!("{order:?} {drains:b}");
                assert_eq!(model.native.presented(), built && ready && wanted, "{case}");
                assert_eq!(model.dismissed, model.presented_retired, "{case}");
                assert_eq!(model.setups, 1, "{case}");
            }
        }
    }

    #[test]
    fn a_stale_apply_for_a_replaced_label_shows_nothing_unbuilt_or_unready() {
        let mut table = WindowTable::default();
        let label = built_window(&mut table, "main", true);
        table.ready(&label);
        table.remove(&label);
        assert_eq!(table.open("main", true, ()), Opened::Build(label.clone()));

        // An apply queued for the window that is gone finds the new entry.
        assert!(table.facts(&label).is_none(), "not built yet");

        table.built(&label);
        let facts = table.facts(&label).unwrap();
        assert!(!facts.actions(false).show, "not ready yet");
        assert!(!facts.actions(false).retire, "and still wanted");
    }

    #[test]
    fn a_close_then_an_open_before_the_apply_ends_wanted_with_nothing_retired() {
        let mut table = WindowTable::default();
        let label = built_window(&mut table, "main", true);
        table.ready(&label);

        assert!(table.unwant(&label));
        assert_eq!(
            table.open("main", true, ()),
            Opened::Existing(label.clone())
        );

        let actions = table.facts(&label).unwrap().actions(true);
        assert!(actions.show);
        assert!(!actions.retire);
        assert!(!actions.dismiss);
    }

    #[test]
    fn a_minimized_window_is_dismissed_like_a_shown_one_and_one_never_shown_is_not() {
        let mut model = Model::new();
        model.event(Event::Built);
        model.event(Event::Ready);
        model.drain_one();
        assert_eq!(model.native, Native::Shown);

        model.event(Event::Minimize);
        model.event(Event::Close);
        model.drain_one();
        assert_eq!(model.dismissed, 1);
        assert_eq!(model.native, Native::Hidden);

        // A second apply finds it hidden.
        model.queued = 1;
        model.drain_one();
        assert_eq!(model.dismissed, 1);

        // Closed before it was ever shown, so there is nothing to tell the kind.
        let mut never = Model::new();
        never.event(Event::Built);
        never.event(Event::Close);
        while never.queued > 0 {
            never.drain_one();
        }
        assert_eq!(never.dismissed, 0);
        assert_eq!(never.native, Native::Hidden);
    }

    #[test]
    fn a_close_before_the_window_is_built_is_acted_on_once_it_is() {
        let mut table = WindowTable::default();
        let Opened::Build(label) = table.open("main", true, ()) else {
            panic!("expected a window to build");
        };

        assert!(table.unwant(&label));
        assert!(!table.unwant(&label), "nothing changed the second time");
        assert!(table.facts(&label).is_none(), "nothing to act on yet");

        assert_eq!(table.built(&label), Some(true));
        assert!(table.facts(&label).unwrap().actions(false).retire);
    }

    #[test]
    fn a_window_built_again_after_it_is_destroyed_starts_from_nothing() {
        let mut table = WindowTable::default();
        let label = built_window(&mut table, "main", true);
        table.ready(&label);
        table.unwant(&label);

        table.remove(&label);

        assert!(!table.wanted(&label));
        assert_eq!(table.open("main", true, ()), Opened::Build(label.clone()));
        assert!(table.wanted(&label));
        assert!(table.facts(&label).is_none());
        table.built(&label);
        let facts = table.facts(&label).unwrap();
        assert!(!facts.ready);
        assert!(facts.wanted);
    }

    #[test]
    fn the_toggle_follows_whether_the_window_is_wanted() {
        let mut table = WindowTable::default();
        let label = built_window(&mut table, "main", true);
        assert!(table.wanted(&label), "opening wants it, even while loading");

        table.unwant(&label);
        assert!(!table.wanted(&label));

        table.open("main", true, ());
        assert!(table.wanted(&label));
        assert!(!table.wanted("missing"));
    }

    #[test]
    fn a_singleton_is_opened_once_and_other_instances_take_the_lowest_free_number() {
        let mut table = WindowTable::default();
        assert_eq!(table.open("main", true, ()), Opened::Build("main".into()));
        assert_eq!(
            table.open("main", true, ()),
            Opened::Existing("main".into())
        );

        for expected in ["editor", "editor-1", "editor-2"] {
            assert_eq!(
                table.open("editor", false, ()),
                Opened::Build(expected.into())
            );
        }
        table.remove("editor-1");
        assert_eq!(
            table.open("editor", false, ()),
            Opened::Build("editor-1".into())
        );
        assert_eq!(table.instances("editor").len(), 3);
    }

    async fn never() {
        std::future::pending::<()>().await
    }

    #[tokio::test]
    async fn ready_stops_a_stored_fallback() {
        let mut table = WindowTable::default();
        let label = built_window(&mut table, "main", true);
        let task = tokio::spawn(never());
        assert!(table.set_watchdog(&label, &task.abort_handle()));

        assert_eq!(table.ready(&label), Some(true));

        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(table.ready(&label), Some(false));
    }

    #[tokio::test]
    async fn remove_stops_a_stored_fallback() {
        let mut table = WindowTable::default();
        let label = built_window(&mut table, "editor", false);
        let task = tokio::spawn(never());
        assert!(table.set_watchdog(&label, &task.abort_handle()));

        table.remove(&label);

        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn a_fallback_for_a_window_that_is_ready_or_gone_is_not_kept() {
        let mut table = WindowTable::default();
        let label = built_window(&mut table, "main", true);
        table.ready(&label);
        let late = tokio::spawn(never());

        assert!(!table.set_watchdog(&label, &late.abort_handle()));
        assert!(!table.set_watchdog("missing", &late.abort_handle()));

        // The table leaves the rejected task to its caller.
        assert!(!late.is_finished());
        late.abort();
        assert!(late.await.unwrap_err().is_cancelled());
    }
}
