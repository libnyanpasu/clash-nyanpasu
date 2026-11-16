//! The reviewed, closed vocabulary behind `permission = "group.name:action"`.
//!
//! A permission resolves through [`GROUPS`] and nowhere else. The `group` and the `action` must
//! both appear in that table, so a typo in either (`remot_admin`, `system.service:instal`) does
//! not compile instead of landing on some default scope. There are no wildcards, inheritance or
//! precedence. `name` is a free label for the resource and carries no authority.
//!
//! The `action` selects the scope class: [`READ_ACTIONS`] grant `read`/`sensitive_read`, every other
//! action grants its group's write scope. The two action sets are disjoint, a group lists the exact
//! actions it accepts, and the effect (`query`) of every action is fixed by the table rather than
//! inferred from the command, so one permission can never describe two scopes or two effects.
//! Adding a group or an action is a reviewed edit to this table and its tests.

use syn::{Error, LitStr};

/// How a read action behaves: the scope it grants and whether the command it labels is a query.
/// Secret-bearing reads are reachable only through `reveal*`; a read-scoped mutation only
/// allocates or advances a resource the caller owns (`follow*`).
const READ_ACTIONS: &[(&str, &str, bool)] = &[
    ("view", "Read", true),
    ("follow", "Read", false),
    ("reveal", "SensitiveRead", true),
    ("reveal_follow", "SensitiveRead", false),
];

struct Group {
    name: &'static str,
    /// The scope of the write actions below. `None` for a group that only has read actions.
    scope: Option<&'static str>,
    /// Write actions that mutate.
    mutations: &'static [&'static str],
    /// Write actions that only read yet need the group's write scope (a latency measurement,
    /// a management listing).
    queries: &'static [&'static str],
    /// The read actions this group accepts. `remote_admin` accepts none, so no read action can
    /// lower administration.
    reads: &'static [&'static str],
}

const fn read_group(name: &'static str, reads: &'static [&'static str]) -> Group {
    Group {
        name,
        scope: None,
        mutations: &[],
        queries: &[],
        reads,
    }
}

#[rustfmt::skip]
const GROUPS: &[Group] = &[
    // Write scopes.
    Group {
        name: "profile", scope: Some("Profile"), reads: &["reveal", "view"], queries: &[],
        mutations: &["activate", "create", "delete", "enhance", "import", "patch", "reorder", "replace", "save", "set", "update"],
    },
    Group {
        name: "proxy", scope: Some("Proxy"), reads: &["reveal", "view"], queries: &["measure"],
        mutations: &["clear", "delete", "healthcheck", "mutate", "select", "set"],
    },
    Group {
        name: "settings", scope: Some("Settings"), reads: &[], queries: &[],
        mutations: &["clear", "create", "patch", "probe", "remove", "report", "set", "test"],
    },
    Group {
        name: "system", scope: Some("System"), reads: &["view"], queries: &[],
        mutations: &[
            "apply", "ask", "cancel", "change", "check", "collect", "copy", "create", "discard", "download", "flush",
            "install", "invoke", "open", "patch", "quit", "report", "restart", "retry", "save", "set", "show",
            "simulate", "start", "stop", "take", "uninstall", "update", "write",
        ],
    },
    Group {
        name: "remote_admin", scope: Some("RemoteAdmin"), reads: &[], queries: &["list", "status"],
        mutations: &["create", "reset", "revoke", "set"],
    },
    // Read-only domains. A sensitive domain lists only `reveal*`.
    read_group("app", &["reveal", "view"]),
    read_group("app_update", &["reveal", "view"]),
    read_group("clash", &["reveal", "view"]),
    read_group("clash_api", &["reveal", "view"]),
    read_group("clash_connection", &["follow"]),
    read_group("clash_ws", &["view"]),
    read_group("clipboard", &["reveal"]),
    read_group("config", &["view"]),
    read_group("configuration", &["reveal", "view"]),
    read_group("core", &["reveal", "view"]),
    read_group("hotkey", &["view"]),
    read_group("http_server", &["view"]),
    read_group("ip_lookup", &["view"]),
    read_group("log", &["follow", "reveal_follow"]),
    read_group("log_file", &["view"]),
    read_group("network_automation", &["reveal"]),
    read_group("rpc", &["view"]),
    read_group("runtime", &["reveal", "view"]),
    read_group("service", &["reveal", "view"]),
    read_group("storage", &["reveal"]),
    read_group("sys_proxy", &["view"]),
    read_group("traffic", &["view"]),
    read_group("transparent_proxy", &["reveal", "view"]),
    read_group("tray_icon", &["view"]),
    read_group("updater", &["reveal", "view"]),
    read_group("widget_icon", &["view"]),
];

fn is_name(part: &str) -> bool {
    part.starts_with(|c: char| c.is_ascii_lowercase())
        && part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The `Scope` variant a permission requires for a command with the declared effect.
pub(super) fn scope(permission: &LitStr, query: bool) -> syn::Result<&'static str> {
    let value = permission.value();
    let fail = |message: String| Err(Error::new(permission.span(), message));
    let parts = value
        .split_once(':')
        .and_then(|(resource, action)| Some((resource.split_once('.')?, action)));
    let Some(((group, name), action)) =
        parts.filter(|((g, n), a)| [g, n, a].into_iter().all(|p| is_name(p)))
    else {
        return fail(format!(
            "permission `{value}` must be `group.name:action` in lower_snake_case"
        ));
    };
    let Some(entry) = GROUPS.iter().find(|entry| entry.name == group) else {
        let groups: Vec<_> = GROUPS.iter().map(|entry| entry.name).collect();
        return fail(format!(
            "permission `{value}` has unknown group `{group}`; expected one of {}",
            groups.join(", ")
        ));
    };
    let (resolved, mutation) = if entry.reads.contains(&action) {
        let &(_, scope, reads_query) = READ_ACTIONS
            .iter()
            .find(|(read, ..)| *read == action)
            .expect("a group may only list read actions");
        (scope, !reads_query)
    } else if let Some(scope) = entry.scope.filter(|_| entry.mutations.contains(&action)) {
        (scope, true)
    } else if let Some(scope) = entry.scope.filter(|_| entry.queries.contains(&action)) {
        (scope, false)
    } else {
        let accepted: Vec<_> = entry
            .reads
            .iter()
            .chain(entry.mutations)
            .chain(entry.queries)
            .collect();
        return fail(format!(
            "permission `{value}` has action `{action}` that group `{group}` does not accept; expected one of {accepted:?} (`{group}.{name}` is only a label)"
        ));
    };
    if mutation == query {
        let expected = if mutation {
            "a mutation, not a `query`"
        } else {
            "a `query`"
        };
        return fail(format!("`{action}` permission `{value}` is for {expected}"));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(permission: &str, query: bool) -> Result<&'static str, String> {
        scope(
            &LitStr::new(permission, proc_macro2::Span::call_site()),
            query,
        )
        .map_err(|e| e.to_string())
    }

    #[test]
    fn groups_and_actions_resolve_to_their_scope() {
        for (permission, query, expected) in [
            ("profile.item:delete", false, "Profile"),
            ("proxy.mode:set", false, "Proxy"),
            ("proxy.url_delay:measure", true, "Proxy"),
            ("settings.storage_item:set", false, "Settings"),
            ("system.service:install", false, "System"),
            ("system.network_automation:apply", false, "System"),
            ("system.wifi_name_access:ask", false, "System"),
            ("network_automation.status:reveal", true, "SensitiveRead"),
            ("remote_admin.token:create", false, "RemoteAdmin"),
            ("remote_admin.token:list", true, "RemoteAdmin"),
            ("profile.listing:view", true, "Read"),
            ("log.session:follow", false, "Read"),
            ("profile.items:reveal", true, "SensitiveRead"),
            ("log.records:reveal_follow", false, "SensitiveRead"),
        ] {
            assert_eq!(resolve(permission, query), Ok(expected), "{permission}");
        }
    }

    #[test]
    fn a_typo_in_a_group_or_an_action_never_resolves() {
        for permission in [
            // The reported cases: a known group with an unknown action, an unknown group with a read action.
            "system.service:typo",
            "remot_admin.tokens:view",
            "remote_admin.tokens:viw",
            // One edit away from every kind of vocabulary entry.
            "systems.service:install",
            "system.service:instal",
            "system.service:installs",
            "profile.item:delet",
            "proxy.mode:sett",
            "settings.storage_item:st",
            "remote_admin.token:revok",
            "corr.version:view",
            "core.version:vew",
            "clipboard.text:reveall",
            "log.session:folow",
            "log.records:reveal_folow",
            "unknown.item:delete",
            "sensitive_read.x:view",
        ] {
            for query in [false, true] {
                assert!(
                    resolve(permission, query).is_err(),
                    "{permission} query={query}"
                );
            }
        }
    }

    #[test]
    fn malformed_permissions_fail_closed() {
        for permission in [
            "",
            "view",
            "profile:view",
            "profile.item",
            ".item:view",
            "profile.:view",
            "profile.item:",
            "profile.item:*",
            "*.*:view",
            "profile.*:view",
            "Profile.item:delete",
            "profile.item:Delete",
            "profile.item:a:b",
            "profile.a.b:view",
            "profile.item: view",
            "1profile.item:view",
        ] {
            for query in [false, true] {
                assert!(
                    resolve(permission, query).is_err(),
                    "{permission:?} query={query}"
                );
            }
        }
    }

    #[test]
    fn effect_must_agree_with_the_permission() {
        for (permission, query) in [
            ("profile.listing:view", false),
            ("profile.items:reveal", false),
            ("log.session:follow", true),
            ("log.records:reveal_follow", true),
            ("profile.item:delete", true),
            ("settings.storage_item:set", true),
            ("system.service:install", true),
            ("proxy.url_delay:measure", false),
            ("remote_admin.token:list", false),
            ("remote_admin.token:create", true),
        ] {
            assert!(
                resolve(permission, query).is_err(),
                "{permission} query={query}"
            );
        }
    }

    #[test]
    fn a_read_action_cannot_lower_administration_or_a_write_scope() {
        for action in READ_ACTIONS.iter().map(|(action, ..)| action) {
            for query in [false, true] {
                assert!(
                    resolve(&format!("remote_admin.token:{action}"), query).is_err(),
                    "{action} {query}"
                );
                // A group that never lists the action rejects it, so `settings` is never readable.
                assert!(
                    resolve(&format!("settings.item:{action}"), query).is_err(),
                    "{action} {query}"
                );
            }
        }
        // A sensitive-only domain cannot be demoted to an ordinary read, and a domain that holds
        // only an ordinary read cannot be promoted by reveal.
        assert!(resolve("clipboard.text:view", true).is_err());
        assert!(resolve("storage.item:view", true).is_err());
        assert!(resolve("traffic.usage:reveal", true).is_err());
    }

    #[test]
    fn the_table_is_consistent_and_unambiguous() {
        let reads: Vec<_> = READ_ACTIONS.iter().map(|(action, ..)| *action).collect();
        let mut names = Vec::new();
        for group in GROUPS {
            assert!(is_name(group.name), "{}", group.name);
            assert!(!names.contains(&group.name), "duplicate {}", group.name);
            names.push(group.name);
            // One scope class per action: write actions and read actions never share a name,
            // and an action is a mutation or a query, never both.
            for action in group.mutations.iter().chain(group.queries) {
                assert!(is_name(action), "{}.{action}", group.name);
                assert!(
                    !reads.contains(action),
                    "{}.{action} shadows a read",
                    group.name
                );
                assert_ne!(
                    group.mutations.contains(action),
                    group.queries.contains(action),
                    "{}.{action}",
                    group.name
                );
                assert!(group.scope.is_some(), "{} has no write scope", group.name);
            }
            for action in group.reads {
                assert!(reads.contains(action), "{}.{action}", group.name);
            }
            // Every group has an explicit reason to exist.
            assert!(
                !group.reads.is_empty() || !group.mutations.is_empty() || !group.queries.is_empty(),
                "{} accepts nothing",
                group.name
            );
            // A group that declares no write action carries no write scope.
            assert_eq!(
                group.scope.is_some(),
                !group.mutations.is_empty() || !group.queries.is_empty(),
                "{}",
                group.name
            );
        }
        // Administration has no read path, and only it holds the `RemoteAdmin` scope.
        let administration: Vec<_> = GROUPS
            .iter()
            .filter(|group| group.scope == Some("RemoteAdmin"))
            .collect();
        assert_eq!(administration.len(), 1);
        assert_eq!(administration[0].name, "remote_admin");
        assert!(administration[0].reads.is_empty());
        // Every group's scope is distinct, so a scope has one writer vocabulary.
        let mut scopes: Vec<_> = GROUPS.iter().filter_map(|group| group.scope).collect();
        scopes.sort_unstable();
        scopes.dedup();
        assert_eq!(scopes.len(), 5);
    }
}
