//! The P0-2 mapping: the requirement, namespace, sensitive marking and exposure every protected
//! resource takes in the permission model, checked against the baseline.
//!
//! The mapping is a committed text table. Its requirement column must equal the reference
//! derivation of design section 3.2 applied to the namespace, and every admission difference
//! between the baseline (scopes) and the mapping (permissions, with callers converted by design
//! section 7) must fall in one of the allowed change categories. The resulting change list and
//! the sensitive resources each role reaches are a committed report, kept fresh by a test.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

use nyanpasu_rpc_protocol::{RpcTarget, Scope};

use super::{Kind, TRANSPORTS, baseline, callers, snake};

const MAPPING: &str = "tests/fixtures/permission-mapping.txt";
const REPORT: &str = "tests/fixtures/permission-mapping-report.txt";
const RESOURCES: [&str; 8] = [
    "meta", "proxy", "profile", "settings", "system", "logs", "traffic", "remote",
];
/// Initial budget of command-level `access =` exceptions (design section 3.3).
const MAX_OVERRIDES: usize = 9;
/// The resources each P4 card registers as derived top-level modules, in order.
const CARDS: [(&str, &[&str]); 5] = [
    ("P4-1", &["meta", "remote"]),
    ("P4-2", &["proxy"]),
    ("P4-3", &["profile"]),
    ("P4-4", &["settings", "system"]),
    ("P4-5", &["logs", "traffic"]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Access {
    Read,
    Write,
}

impl Access {
    fn parse(text: &str) -> Self {
        match text {
            "read" => Self::Read,
            "write" => Self::Write,
            other => panic!("unknown access {other}"),
        }
    }
}

/// A permission: a domain resource and an access level.
type Permission = (&'static str, Access);
/// What a caller holds: at most one access level per domain resource.
type Permissions = BTreeMap<&'static str, Access>;

#[derive(Debug, Clone)]
struct Mapped {
    permission: Permission,
    namespace: String,
    sensitive: bool,
    override_access: Option<Access>,
    targets: Vec<String>,
    origin: String,
}

fn resource(name: &str) -> &'static str {
    RESOURCES
        .iter()
        .find(|resource| **resource == name)
        .unwrap_or_else(|| panic!("{name} is not a domain resource"))
}

fn mapping() -> BTreeMap<(Kind, String), Mapped> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MAPPING);
    let text = fs::read_to_string(path).expect("the permission mapping must be readable");
    let mut table = BTreeMap::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        assert_eq!(columns.len(), 8, "malformed row: {line}");
        let kind = match columns[0] {
            "command" => Kind::Command,
            "event" => Kind::Event,
            "route" => Kind::Route,
            "stream" => Kind::Stream,
            other => panic!("unknown resource kind {other}"),
        };
        let (domain, access) = columns[2]
            .split_once(':')
            .unwrap_or_else(|| panic!("malformed permission: {line}"));
        let mapped = Mapped {
            permission: (resource(domain), Access::parse(access)),
            namespace: columns[3].to_owned(),
            sensitive: match columns[4] {
                "sensitive" => true,
                "" => false,
                other => panic!("unknown marking {other}"),
            },
            override_access: (!columns[5].is_empty()).then(|| Access::parse(columns[5])),
            targets: columns[6].split(',').map(str::to_owned).collect(),
            origin: columns[7].to_owned(),
        };
        let previous = table.insert((kind, columns[1].to_owned()), mapped);
        assert!(previous.is_none(), "duplicate row: {line}");
    }
    table
}

/// The reference derivation of design section 3.2: the resource is the top-level module, the
/// level is the effect unless the command overrides it, and events, routes and streams read.
fn derive(kind: Kind, effect: &str, mapped: &Mapped) -> Permission {
    let top = mapped.namespace.split('.').next().unwrap();
    let access = match (kind, mapped.override_access) {
        (Kind::Command, Some(access)) => access,
        (Kind::Command, None) if effect == "mutation" => Access::Write,
        _ => Access::Read,
    };
    (resource(top), access)
}

/// Design section 7: the permissions an old scope converts to.
fn convert(scope: Scope) -> &'static [Permission] {
    const READS: &[Permission] = &[
        ("proxy", Access::Read),
        ("profile", Access::Read),
        ("settings", Access::Read),
        ("system", Access::Read),
        ("logs", Access::Read),
        ("traffic", Access::Read),
    ];
    match scope {
        Scope::Read => READS,
        Scope::SensitiveRead => &[],
        Scope::Proxy => &[("proxy", Access::Write)],
        Scope::Profile => &[("profile", Access::Write)],
        Scope::Settings => &[
            ("settings", Access::Write),
            ("logs", Access::Write),
            ("traffic", Access::Write),
        ],
        Scope::System => &[("system", Access::Write)],
        Scope::RemoteAdmin => &[("remote", Access::Write)],
    }
}

fn grant(permissions: &mut Permissions, (resource, access): Permission) {
    let held = permissions.entry(resource).or_insert(access);
    *held = (*held).max(access);
}

/// The permissions a resolved caller holding `scopes` has in the new model.
fn converted(scopes: &BTreeSet<Scope>) -> Permissions {
    let mut permissions = Permissions::from([("meta", Access::Read)]);
    for scope in scopes {
        for permission in convert(*scope) {
            grant(&mut permissions, *permission);
        }
    }
    permissions
}

fn permits(permissions: &Permissions, (resource, access): Permission) -> bool {
    permissions
        .get(resource)
        .is_some_and(|held| *held >= access)
}

/// Design section 2.2: what each role expands to.
fn role(name: &str) -> Permissions {
    let mut permissions = Permissions::from([("meta", Access::Read)]);
    let tiers: &[(&str, &[Permission])] = &[
        ("anonymous", &[]),
        (
            "viewer",
            &[
                ("proxy", Access::Read),
                ("profile", Access::Read),
                ("settings", Access::Read),
                ("system", Access::Read),
                ("logs", Access::Read),
                ("traffic", Access::Read),
            ],
        ),
        (
            "operator",
            &[
                ("proxy", Access::Write),
                ("profile", Access::Write),
                ("traffic", Access::Write),
                ("settings", Access::Write),
            ],
        ),
        (
            "admin",
            &[("system", Access::Write), ("logs", Access::Write)],
        ),
        ("owner", &[("remote", Access::Write)]),
    ];
    for (tier, added) in tiers {
        for permission in *added {
            grant(&mut permissions, *permission);
        }
        if *tier == name {
            return permissions;
        }
    }
    panic!("unknown role {name}")
}

const ROLES: [&str; 5] = ["anonymous", "viewer", "operator", "admin", "owner"];

/// The allowed change categories of design section 7 that a mapping can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Change {
    /// Category 1: the resource required `sensitive_read` and now requires its domain's read.
    SensitiveRead,
    /// Category 2: a caller holding only the domain's write may now read.
    WriteIncludesRead,
}

impl Change {
    fn label(self) -> &'static str {
        match self {
            Self::SensitiveRead => "1 sensitive read no longer guarded",
            Self::WriteIncludesRead => "2 write includes read",
        }
    }
}

/// The category an admission difference belongs to, or `None` when no category explains it.
fn classify(
    old_scope: &str,
    permission: Permission,
    scopes: &BTreeSet<Scope>,
    now_admitted: bool,
) -> Option<Change> {
    if old_scope == Scope::SensitiveRead.as_str() {
        return Some(Change::SensitiveRead);
    }
    let (resource, access) = permission;
    let held_write = converted(scopes).get(resource) == Some(&Access::Write);
    (now_admitted && access == Access::Read && held_write).then_some(Change::WriteIncludesRead)
}

struct Comparison {
    /// Per resource, the categories its admission differences fall in.
    changes: BTreeMap<(Kind, String), BTreeSet<Change>>,
    /// Admission differences no category explains.
    unexplained: Vec<String>,
}

fn compare() -> Comparison {
    let before = baseline();
    let after = mapping();
    let callers = callers();
    let mut changes = BTreeMap::<_, BTreeSet<_>>::new();
    let mut unexplained = Vec::new();
    for (key @ (kind, name), row) in &before {
        let mapped = &after[key];
        for transport in TRANSPORTS {
            let exposed = row.targets.contains(&snake(transport));
            for caller in &callers {
                let old = super::admits(*kind, name, row, transport, caller);
                let new = caller.as_ref().is_some_and(|scopes| {
                    exposed && permits(&converted(scopes), mapped.permission)
                });
                if old == new {
                    continue;
                }
                let scopes = caller.as_ref().expect("unresolved callers never differ");
                match classify(&row.scope, mapped.permission, scopes, new) {
                    Some(change) => {
                        changes.entry(key.clone()).or_default().insert(change);
                    }
                    None => unexplained.push(format!(
                        "{} {name} on {}: {:?} {}",
                        snake(kind),
                        snake(transport),
                        scopes,
                        if new { "now admitted" } else { "now refused" }
                    )),
                }
            }
        }
    }
    Comparison {
        changes,
        unexplained,
    }
}

/// The committed report: the change list and, per role, the sensitive resources it reaches.
fn report() -> String {
    let mapped = mapping();
    let comparison = compare();
    let mut output = String::from(
        "# Generated by export_permission_mapping_report; do not edit.\n\
         ## Change list: kind | name | old scope | new permission | categories\n",
    );
    let old = baseline();
    for (key @ (kind, name), changes) in &comparison.changes {
        let (resource, access) = mapped[key].permission;
        let categories: Vec<_> = changes.iter().map(|change| change.label()).collect();
        writeln!(
            output,
            "{} | {name} | {} | {resource}:{} | {}",
            snake(kind),
            old[key].scope,
            snake(access),
            categories.join("; ")
        )
        .unwrap();
    }
    for role_name in ROLES {
        let permissions = role(role_name);
        let reached: Vec<_> = mapped
            .iter()
            .filter(|(_, mapped)| mapped.sensitive && permits(&permissions, mapped.permission))
            .map(|((kind, name), _)| format!("{} {name}", snake(kind)))
            .collect();
        writeln!(
            output,
            "## Sensitive resources {role_name} reaches ({}): {}",
            reached.len(),
            reached.join(", ")
        )
        .unwrap();
    }
    output
}

fn report_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(REPORT)
}

#[test]
#[ignore = "explicit generator; the freshness test never writes"]
fn export_permission_mapping_report() {
    fs::write(report_path(), report()).expect("the mapping report must be writable");
}

#[test]
fn mapping_report_is_fresh() {
    let committed = fs::read_to_string(report_path()).unwrap_or_default();
    assert!(
        committed == report(),
        "the mapping report is stale; run export_permission_mapping_report explicitly"
    );
}

#[test]
fn mapping_covers_the_baseline_with_its_exposure() {
    let before = baseline();
    let after = mapping();
    let mapped: BTreeSet<_> = after.keys().collect();
    assert_eq!(mapped, before.keys().collect());
    for (key, row) in &before {
        assert_eq!(after[key].targets, row.targets, "{key:?}");
    }
}

#[test]
fn requirements_follow_the_reference_derivation() {
    let before = baseline();
    let mut overrides = 0;
    for ((kind, name), mapped) in mapping() {
        let effect = &before[&(kind, name.clone())].effect;
        assert_eq!(derive(kind, effect, &mapped), mapped.permission, "{name}");
        if let Some(access) = mapped.override_access {
            overrides += 1;
            assert_eq!(kind, Kind::Command, "{name}: only commands override");
            let natural = if effect == "mutation" {
                Access::Write
            } else {
                Access::Read
            };
            assert_ne!(access, natural, "{name}: an override must change the level");
        }
    }
    assert!(overrides <= MAX_OVERRIDES, "{overrides} overrides");
}

#[test]
fn meta_and_remote_keep_their_boundaries() {
    let mapping = mapping();
    let meta: Vec<_> = mapping
        .iter()
        .filter(|(_, mapped)| mapped.permission.0 == "meta")
        .map(|((_, name), _)| name.as_str())
        .collect();
    assert_eq!(meta, ["rpc_describe"]);
    for ((_, name), mapped) in &mapping {
        if mapped.permission.0 == "remote" {
            assert!(!mapped.targets.contains(&snake(RpcTarget::Http)), "{name}");
        }
    }
}

#[test]
fn sensitive_markings_carry_over_every_sensitive_read() {
    let before = baseline();
    for (key, mapped) in mapping() {
        let name = &key.1;
        let was_sensitive = before[&key].scope == Scope::SensitiveRead.as_str();
        assert_eq!(mapped.sensitive, was_sensitive, "{name}");
        if mapped.sensitive {
            assert_eq!(mapped.permission.1, Access::Read, "{name}");
        }
    }
}

#[test]
fn every_admission_difference_has_a_category() {
    let comparison = compare();
    assert!(
        comparison.unexplained.is_empty(),
        "{}",
        comparison.unexplained.join("\n")
    );
}

/// The derived and legacy declarations are separated by position: when a card registers a
/// top-level module, an existing source module of the same name may only hold resources that
/// belong to a resource registered by then.
#[test]
fn registered_top_modules_hold_no_legacy_declarations() {
    let mut registered = BTreeSet::new();
    for (card, resources) in CARDS {
        registered.extend(resources.iter().copied());
        for ((_, name), mapped) in mapping() {
            let stem = Path::new(&mapped.origin)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap();
            if registered.contains(stem) {
                assert!(
                    registered.contains(mapped.permission.0),
                    "{card}: {name} in {} belongs to {}, which is not registered yet",
                    mapped.origin,
                    mapped.permission.0
                );
            }
        }
    }
    let all: BTreeSet<_> = RESOURCES.into_iter().collect();
    assert_eq!(registered, all);
}

#[test]
fn roles_expand_like_the_design() {
    let expect =
        |pairs: &[(&'static str, Access)]| -> Permissions { pairs.iter().copied().collect() };
    use Access::{Read as R, Write as W};
    assert_eq!(role("anonymous"), expect(&[("meta", R)]));
    assert_eq!(
        role("viewer"),
        expect(&[
            ("meta", R),
            ("proxy", R),
            ("profile", R),
            ("settings", R),
            ("system", R),
            ("logs", R),
            ("traffic", R),
        ])
    );
    assert_eq!(
        role("operator"),
        expect(&[
            ("meta", R),
            ("proxy", W),
            ("profile", W),
            ("settings", W),
            ("system", R),
            ("logs", R),
            ("traffic", W),
        ])
    );
    assert_eq!(
        role("admin"),
        expect(&[
            ("meta", R),
            ("proxy", W),
            ("profile", W),
            ("settings", W),
            ("system", W),
            ("logs", W),
            ("traffic", W),
        ])
    );
    let mut owner = role("admin");
    owner.insert("remote", W);
    assert_eq!(role("owner"), owner);
}

#[test]
fn comparison_flags_a_requirement_outside_the_categories() {
    let changed = classify(
        "system",
        ("settings", Access::Write),
        &BTreeSet::from([Scope::Settings]),
        true,
    );
    assert_eq!(changed, None);
}
