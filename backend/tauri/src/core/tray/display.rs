//! What the tray displays, kept as one record that only a complete
//! publication replaces.

use super::proxies::{TrayProxies, TrayUpdateType, diff_proxies};

/// The proxy section of a built menu: the groups and selections it shows.
#[derive(Default)]
pub(super) struct ProxySection {
    pub(super) proxies: TrayProxies,
}

/// A published menu, as the tray displays it. `M` is the menu the tray icon
/// holds.
pub(super) struct Published<M> {
    /// `None` until the first build attaches one.
    attached: Option<M>,
    section: ProxySection,
}

/// What the tray displays.
pub(super) enum TrayDisplay<M> {
    /// Exactly this. Replaced whole, and only by a publication that completed
    /// every step.
    Known(Published<M>),
    /// A failure after the live menu may have changed left what it shows
    /// unknown. Nothing is diffed with it, and the next update rebuilds.
    /// `attached` is the menu object the icon still holds, which a Linux
    /// rebuild refills.
    Unknown { attached: Option<M> },
}

/// How an attempt to publish a candidate menu ended.
pub(super) enum Publication<M> {
    /// The candidate could not be built, so the live tray was not touched.
    NotStarted,
    /// Every step succeeded: the icon holds `attached`, showing `section`.
    Complete { attached: M, section: ProxySection },
    /// A step failed after the live tray may have changed. The icon holds
    /// `attached`, as far as anything is known.
    Interrupted { attached: Option<M> },
}

/// How much of a checkmark repaint took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shown {
    Whole,
    /// Only part of it, or none.
    Partly,
}

/// What a repaint may paint.
pub(super) enum Paint<M> {
    Menu(M),
    /// Nothing is built yet, so there is nothing to repaint or lose.
    NotBuilt,
    /// The menu is unknown: rebuild it instead.
    Unknown,
}

impl<M: Clone> TrayDisplay<M> {
    /// Nothing is displayed before the first build.
    pub(super) fn new() -> Self {
        Self::Known(Published {
            attached: None,
            section: ProxySection::default(),
        })
    }

    /// The menu object the icon holds, whatever is known of its contents.
    pub(super) fn attached(&self) -> Option<M> {
        match self {
            Self::Known(published) => published.attached.clone(),
            Self::Unknown { attached } => attached.clone(),
        }
    }

    pub(super) fn published(&mut self, publication: Publication<M>) {
        match publication {
            Publication::NotStarted => {}
            Publication::Complete { attached, section } => {
                *self = Self::Known(Published {
                    attached: Some(attached),
                    section,
                });
            }
            Publication::Interrupted { attached } => *self = Self::Unknown { attached },
        }
    }

    /// How to bring the proxy section up to `current`.
    pub(super) fn update_to(&self, current: &TrayProxies) -> TrayUpdateType {
        match self {
            Self::Known(published) => diff_proxies(&published.section.proxies, current),
            Self::Unknown { .. } => TrayUpdateType::Full,
        }
    }

    pub(super) fn paint(&self) -> Paint<M> {
        match self {
            Self::Known(Published {
                attached: Some(menu),
                ..
            }) => Paint::Menu(menu.clone()),
            Self::Known(_) => Paint::NotBuilt,
            Self::Unknown { .. } => Paint::Unknown,
        }
    }

    /// A checkmark repaint ended; `proxies` is the proxy selection it
    /// painted, if it painted one. Anything short of all of it leaves the
    /// menu unknown.
    pub(super) fn repainted(&mut self, proxies: Option<TrayProxies>, shown: Shown) {
        let Self::Known(published) = self else {
            return;
        };
        match shown {
            Shown::Whole => {
                if let Some(proxies) = proxies {
                    published.section.proxies = proxies;
                }
            }
            Shown::Partly => {
                *self = Self::Unknown {
                    attached: published.attached.take(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{super::proxies::TrayGroup, *};

    fn selecting(proxy: &str) -> TrayProxies {
        TrayProxies::from([(
            "Proxy".to_owned(),
            TrayGroup {
                now: Some(proxy.to_owned()),
                all: vec!["a".to_owned(), "b".to_owned()],
                selectable: true,
            },
        )])
    }

    fn section(selected: &str) -> ProxySection {
        ProxySection {
            proxies: selecting(selected),
        }
    }

    fn switching(from: &str, to: &str) -> TrayUpdateType {
        TrayUpdateType::Part(vec![("Proxy".to_owned(), from.to_owned(), to.to_owned())])
    }

    /// The menu "old" is published, showing `a`.
    fn known() -> TrayDisplay<&'static str> {
        let mut display = TrayDisplay::new();
        display.published(Publication::Complete {
            attached: "old",
            section: section("a"),
        });
        display
    }

    fn assert_unknown(display: &TrayDisplay<&'static str>, attached: Option<&'static str>) {
        assert!(matches!(display, TrayDisplay::Unknown { .. }));
        assert_eq!(display.attached(), attached);
        assert_eq!(display.update_to(&selecting("a")), TrayUpdateType::Full);
        assert!(matches!(display.paint(), Paint::Unknown));
    }

    #[test]
    fn nothing_is_displayed_before_the_first_build() {
        let display = TrayDisplay::<&str>::new();
        assert!(matches!(display.paint(), Paint::NotBuilt));
        assert_eq!(display.update_to(&TrayProxies::new()), TrayUpdateType::None);
        assert_eq!(display.update_to(&selecting("a")), TrayUpdateType::Full);
    }

    #[test]
    fn a_complete_publication_replaces_the_record_whole() {
        let display = known();
        assert!(matches!(display.paint(), Paint::Menu("old")));
        assert_eq!(display.update_to(&selecting("a")), TrayUpdateType::None);
        assert_eq!(display.update_to(&selecting("b")), switching("a", "b"));
    }

    /// The candidate could not be built, so the tray still shows the old menu
    /// and the old record still describes it.
    #[test]
    fn a_candidate_that_failed_to_build_keeps_the_known_record() {
        let mut display = known();
        display.published(Publication::NotStarted);
        assert!(matches!(display.paint(), Paint::Menu("old")));
        assert_eq!(display.update_to(&selecting("b")), switching("a", "b"));
    }

    /// `set_menu` attached the candidate, then making the tray visible failed.
    #[test]
    fn a_visibility_failure_after_set_menu_leaves_the_menu_unknown() {
        let mut display = known();
        display.published(Publication::Interrupted {
            attached: Some("new"),
        });
        assert_unknown(&display, Some("new"));
    }

    /// Linux refills the menu object it holds in place; some items could not
    /// be replaced and the publication returned early. The object stays the
    /// one to refill, but the record no longer describes what it shows.
    #[test]
    fn a_partial_linux_refill_leaves_the_menu_unknown() {
        let mut display = known();
        display.published(Publication::Interrupted {
            attached: Some("old"),
        });
        assert_unknown(&display, Some("old"));
    }

    /// A checkmark repaint that took only in part: recording its target would
    /// make the next update of the same snapshot a no-op that never repairs
    /// the menu.
    #[test]
    fn a_partial_repaint_leaves_the_menu_unknown() {
        let mut display = known();
        display.repainted(Some(selecting("b")), Shown::Partly);
        assert_unknown(&display, Some("old"));
    }

    #[test]
    fn a_whole_repaint_moves_only_the_selection() {
        let mut display = known();
        display.repainted(Some(selecting("b")), Shown::Whole);
        assert_eq!(display.update_to(&selecting("b")), TrayUpdateType::None);

        display.repainted(None, Shown::Whole);
        assert_eq!(display.update_to(&selecting("b")), TrayUpdateType::None);
    }

    #[test]
    fn a_complete_rebuild_makes_an_unknown_menu_known_again() {
        let mut display = known();
        display.published(Publication::Interrupted {
            attached: Some("new"),
        });
        display.published(Publication::Complete {
            attached: "new",
            section: section("b"),
        });
        assert!(matches!(display.paint(), Paint::Menu("new")));
        assert_eq!(display.update_to(&selecting("b")), TrayUpdateType::None);
    }
}
