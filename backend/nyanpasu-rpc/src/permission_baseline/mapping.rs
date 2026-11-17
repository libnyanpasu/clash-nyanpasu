//! The P0-2 mapping: the requirement, namespace, sensitive marking and exposure every protected
//! resource takes in the permission model, checked against the baseline.
//!
//! The mapping is a committed text table. Its requirement column must equal the reference
//! derivation of design section 3.2 applied to the namespace, and every admission difference
//! between the baseline (scopes) and the mapping (permissions, with callers converted by design
//! section 7) must fall in one of the allowed change categories. The resulting change list and
//! the sensitive resources each role reaches are a committed report, kept fresh by a test. The
//! protocol's requirement table is generated from the mapping the same way.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

use nyanpasu_config::application::{Access, Permission, Permissions, Resource, Role};
use nyanpasu_rpc_protocol::{RpcTarget, Scope};

use super::{Kind, TRANSPORTS, baseline, callers, snake};

const MAPPING: &str = "tests/fixtures/permission-mapping.txt";
const REPORT: &str = "tests/fixtures/permission-mapping-report.txt";
const REQUIREMENTS: &str = "../nyanpasu-rpc-protocol/src/permission/requirements.rs";
/// Initial budget of command-level `access =` exceptions (design section 3.3).
const MAX_OVERRIDES: usize = 9;
/// The resources each P4 card registers as derived top-level modules, in order.
const CARDS: [(&str, &[Resource]); 5] = [
    ("P4-1", &[Resource::Meta, Resource::Remote]),
    ("P4-2", &[Resource::Proxy]),
    ("P4-3", &[Resource::Profile]),
    ("P4-4", &[Resource::Settings, Resource::System]),
    ("P4-5", &[Resource::Logs, Resource::Traffic]),
];
const ROLES: [Role; 5] = [
    Role::Anonymous,
    Role::Viewer,
    Role::Operator,
    Role::Admin,
    Role::Owner,
];

#[derive(Debug, Clone)]
struct Mapped {
    permission: Permission,
    namespace: String,
    sensitive: bool,
    override_access: Option<Access>,
    targets: Vec<String>,
    origin: String,
}

fn parse<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_value(serde_json::Value::from(name))
        .unwrap_or_else(|error| panic!("{name}: {error}"))
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
            permission: Permission::new(parse(domain), parse(access)),
            namespace: columns[3].to_owned(),
            sensitive: match columns[4] {
                "sensitive" => true,
                "" => false,
                other => panic!("unknown marking {other}"),
            },
            override_access: (!columns[5].is_empty()).then(|| parse(columns[5])),
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
    Permission::new(parse(top), access)
}

/// Design section 7: the permissions an old scope converts to. Only comparisons and tests use
/// it; nothing persisted needs converting.
fn convert(scope: Scope) -> Vec<Permission> {
    use Access::{Read, Write};
    use Resource::{Logs, Profile, Proxy, Remote, Settings, System, Traffic};
    let pairs: &[(Resource, Access)] = match scope {
        Scope::Read => &[
            (Proxy, Read),
            (Profile, Read),
            (Settings, Read),
            (System, Read),
            (Logs, Read),
            (Traffic, Read),
        ],
        Scope::SensitiveRead => &[],
        Scope::Proxy => &[(Proxy, Write)],
        Scope::Profile => &[(Profile, Write)],
        Scope::Settings => &[(Settings, Write), (Logs, Write), (Traffic, Write)],
        Scope::System => &[(System, Write)],
        Scope::RemoteAdmin => &[(Remote, Write)],
    };
    pairs
        .iter()
        .map(|(resource, access)| Permission::new(*resource, *access))
        .collect()
}

/// The permissions a resolved caller holding `scopes` has in the new model.
fn converted(scopes: &BTreeSet<Scope>) -> Permissions {
    scopes.iter().flat_map(|scope| convert(*scope)).fold(
        Permissions::NONE,
        |permissions, permission| {
            let access = permissions
                .get(permission.resource)
                .map_or(permission.access, |held| held.max(permission.access));
            permissions.with(permission.resource, access)
        },
    )
}

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
    let held_write = converted(scopes).get(permission.resource) == Some(Access::Write);
    (now_admitted && permission.access == Access::Read && held_write)
        .then_some(Change::WriteIncludesRead)
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
                let new = caller
                    .as_ref()
                    .is_some_and(|scopes| exposed && converted(scopes).permits(mapped.permission));
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
        let categories: Vec<_> = changes.iter().map(|change| change.label()).collect();
        writeln!(
            output,
            "{} | {name} | {} | {} | {}",
            snake(kind),
            old[key].scope,
            mapped[key].permission,
            categories.join("; ")
        )
        .unwrap();
    }
    for role in ROLES {
        let permissions = role.permissions();
        let reached: Vec<_> = mapped
            .iter()
            .filter(|(_, mapped)| mapped.sensitive && permissions.permits(mapped.permission))
            .map(|((kind, name), _)| format!("{} {name}", snake(kind)))
            .collect();
        writeln!(
            output,
            "## Sensitive resources {role} reaches ({}): {}",
            reached.len(),
            reached.join(", ")
        )
        .unwrap();
    }
    output
}

/// The protocol's committed requirement table: each resource's requirement and sensitive
/// marking, in name order.
fn requirements() -> String {
    let variant = |value: &dyn std::fmt::Display| -> String {
        value
            .to_string()
            .split('_')
            .map(|part| part[..1].to_uppercase() + &part[1..])
            .collect()
    };
    let mut rows: Vec<_> = mapping()
        .into_iter()
        .map(|((_, name), mapped)| (name, mapped))
        .collect();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    let mut output = String::from(
        "// Generated by export_permission_requirements from the P0-2 mapping; do not edit.\n\n\
         use nyanpasu_config::application::{Access, Permission, Resource};\n\n\
         use super::Requirement;\n\n\
         /// The requirement and sensitive marking of every resource in the permission model.\n\
         #[rustfmt::skip]\n\
         pub const REQUIREMENTS: &[Requirement] = &[\n",
    );
    for (name, mapped) in rows {
        writeln!(
            output,
            "    Requirement {{ name: {name:?}, permission: Permission::new(Resource::{}, Access::{}), sensitive: {} }},",
            variant(&mapped.permission.resource),
            variant(&mapped.permission.access),
            mapped.sensitive
        )
        .unwrap();
    }
    output.push_str("];\n");
    output
}

fn path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
#[ignore = "explicit generator; the freshness test never writes"]
fn export_permission_mapping_report() {
    fs::write(path(REPORT), report()).expect("the mapping report must be writable");
}

#[test]
fn mapping_report_is_fresh() {
    let committed = fs::read_to_string(path(REPORT)).unwrap_or_default();
    assert!(
        committed == report(),
        "the mapping report is stale; run export_permission_mapping_report explicitly"
    );
}

#[test]
#[ignore = "explicit generator; the freshness test never writes"]
fn export_permission_requirements() {
    fs::write(path(REQUIREMENTS), requirements()).expect("the requirement table must be writable");
}

#[test]
fn permission_requirements_are_fresh() {
    let committed = fs::read_to_string(path(REQUIREMENTS)).unwrap_or_default();
    assert!(
        committed.replace("\r\n", "\n") == requirements(),
        "the requirement table is stale; run export_permission_requirements explicitly"
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
        .filter(|(_, mapped)| mapped.permission.resource == Resource::Meta)
        .map(|((_, name), _)| name.as_str())
        .collect();
    assert_eq!(meta, ["rpc_describe"]);
    for ((_, name), mapped) in &mapping {
        if mapped.permission.resource == Resource::Remote {
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
            assert_eq!(mapped.permission.access, Access::Read, "{name}");
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
        registered.extend(resources.iter().map(ToString::to_string));
        for ((_, name), mapped) in mapping() {
            let stem = Path::new(&mapped.origin)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap();
            let owner = mapped.permission.resource.to_string();
            if registered.contains(stem) {
                assert!(
                    registered.contains(&owner),
                    "{card}: {name} in {} belongs to {owner}, which is not registered yet",
                    mapped.origin,
                );
            }
        }
    }
    // The owner writes every resource but meta.
    let all: BTreeSet<_> = Role::Owner
        .permissions()
        .iter()
        .map(|(resource, _)| resource)
        .chain([Resource::Meta])
        .map(|resource| resource.to_string())
        .collect();
    assert_eq!(registered, all);
}

#[test]
fn comparison_flags_a_requirement_outside_the_categories() {
    let changed = classify(
        "system",
        Permission::new(Resource::Settings, Access::Write),
        &BTreeSet::from([Scope::Settings]),
        true,
    );
    assert_eq!(changed, None);
}

/// The test-only conversion of design section 7, on the permission model's own types.
#[test]
fn legacy_scopes_convert_to_the_design_permissions() {
    use Access::Write;
    assert_eq!(
        converted(&BTreeSet::from([Scope::Read])),
        Role::Viewer.permissions()
    );
    assert_eq!(
        converted(&BTreeSet::from([Scope::SensitiveRead])),
        Permissions::NONE
    );
    assert_eq!(
        converted(&BTreeSet::from([Scope::Read, Scope::Settings])),
        Role::Viewer
            .permissions()
            .with(Resource::Settings, Write)
            .with(Resource::Logs, Write)
            .with(Resource::Traffic, Write)
    );
    assert_eq!(
        converted(&Scope::ALL.into_iter().collect()),
        Role::Owner.permissions()
    );
    assert_eq!(
        converted(&BTreeSet::from([Scope::Proxy])).get(Resource::Proxy),
        Some(Write)
    );
}
