//! The P0-2 mapping: the requirement, namespace, sensitive marking and exposure every protected
//! resource takes in the permission model, checked against the scope baseline it replaced.
//!
//! The mapping is a committed text table. Its requirement column must equal the reference
//! derivation of design section 3.2 applied to the namespace, and every admission difference
//! between the frozen scope baseline and the mapping (permissions, with callers converted by
//! design section 7) must fall in one of the allowed change categories. The resulting change list
//! and the sensitive resources each role reaches are a committed report, kept fresh by a test.
//! The resource table must hold exactly the mapping: that is what the switch put in force.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

use nyanpasu_config::application::{Access, Permission, Permissions, Resource, Role};
use nyanpasu_rpc_protocol::RpcTarget;

use super::{Kind, TRANSPORTS, current, kind_name, list, parse_name, parse_permission, snake};

const MAPPING: &str = "tests/fixtures/permission-mapping.txt";
const REPORT: &str = "tests/fixtures/permission-mapping-report.txt";
/// The scope table in force before the switch, frozen.
const SCOPE_BASELINE: &str = "tests/fixtures/permission-scope-baseline.txt";
/// Every scope of the replaced model, in its declaration order.
const SCOPES: [&str; 7] = [
    "read",
    "sensitive_read",
    "proxy",
    "profile",
    "settings",
    "system",
    "remote_admin",
];
/// The one command any resolved caller could invoke without holding its scope.
const SELF_INSPECTION: &str = "rpc_describe";
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

/// One resource as the scope model authorized it.
#[derive(Debug, Clone)]
struct ScopeRow {
    scope: &'static str,
    effect: String,
    targets: Vec<String>,
}

fn scope_baseline() -> BTreeMap<(Kind, String), ScopeRow> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCOPE_BASELINE);
    let text = fs::read_to_string(path).expect("the scope baseline must be readable");
    let mut table = BTreeMap::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        assert_eq!(columns.len(), 7, "malformed row: {line}");
        let scope = SCOPES
            .into_iter()
            .find(|scope| *scope == columns[2])
            .unwrap_or_else(|| panic!("unknown scope: {line}"));
        let row = ScopeRow {
            scope,
            effect: columns[3].to_owned(),
            targets: list(columns[4]),
        };
        let previous = table.insert((kind_name(columns[0]), columns[1].to_owned()), row);
        assert!(previous.is_none(), "duplicate row: {line}");
    }
    table
}

/// A caller of the scope model: unresolved, or resolved with these scopes.
type ScopeCaller = Option<BTreeSet<&'static str>>;

/// The unresolved caller and every subset of the scopes.
fn scope_callers() -> Vec<ScopeCaller> {
    let mut callers = vec![None];
    for mask in 0..1_u32 << SCOPES.len() {
        let scopes = SCOPES
            .iter()
            .enumerate()
            .filter(|(bit, _)| mask & (1 << bit) != 0)
            .map(|(_, scope)| *scope)
            .collect();
        callers.push(Some(scopes));
    }
    callers
}

/// How the scope model admitted: authentication, exposure, then the scope, with the
/// self-inspection exception.
fn scope_admits(
    kind: Kind,
    name: &str,
    row: &ScopeRow,
    transport: RpcTarget,
    caller: &ScopeCaller,
) -> bool {
    let Some(scopes) = caller else { return false };
    row.targets.contains(&snake(transport))
        && ((kind == Kind::Command && name == SELF_INSPECTION) || scopes.contains(row.scope))
}

fn mapping() -> BTreeMap<(Kind, String), Mapped> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MAPPING);
    let text = fs::read_to_string(path).expect("the permission mapping must be readable");
    let mut table = BTreeMap::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        assert_eq!(columns.len(), 8, "malformed row: {line}");
        let kind = kind_name(columns[0]);
        let mapped = Mapped {
            permission: parse_permission(columns[2]),
            namespace: columns[3].to_owned(),
            sensitive: match columns[4] {
                "sensitive" => true,
                "" => false,
                other => panic!("unknown marking {other}"),
            },
            override_access: (!columns[5].is_empty()).then(|| parse_name(columns[5])),
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
    Permission::new(parse_name(top), access)
}

/// Design section 7: the permissions an old scope converts to. Only comparisons and tests use
/// it; nothing persisted needs converting.
fn convert(scope: &str) -> Vec<Permission> {
    use Access::{Read, Write};
    use Resource::{Logs, Profile, Proxy, Remote, Settings, System, Traffic};
    let pairs: &[(Resource, Access)] = match scope {
        "read" => &[
            (Proxy, Read),
            (Profile, Read),
            (Settings, Read),
            (System, Read),
            (Logs, Read),
            (Traffic, Read),
        ],
        "sensitive_read" => &[],
        "proxy" => &[(Proxy, Write)],
        "profile" => &[(Profile, Write)],
        "settings" => &[(Settings, Write), (Logs, Write), (Traffic, Write)],
        "system" => &[(System, Write)],
        "remote_admin" => &[(Remote, Write)],
        other => panic!("unknown scope {other}"),
    };
    pairs
        .iter()
        .map(|(resource, access)| Permission::new(*resource, *access))
        .collect()
}

/// The permissions a resolved caller holding `scopes` has in the new model.
fn converted(scopes: &BTreeSet<&'static str>) -> Permissions {
    scopes.iter().flat_map(|scope| convert(scope)).fold(
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
    scopes: &BTreeSet<&'static str>,
    now_admitted: bool,
) -> Option<Change> {
    if old_scope == "sensitive_read" {
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
    let before = scope_baseline();
    let after = mapping();
    let callers = scope_callers();
    let mut changes = BTreeMap::<_, BTreeSet<_>>::new();
    let mut unexplained = Vec::new();
    for (key @ (kind, name), row) in &before {
        let mapped = &after[key];
        for transport in TRANSPORTS {
            let exposed = row.targets.contains(&snake(transport));
            for caller in &callers {
                let old = scope_admits(*kind, name, row, transport, caller);
                let new = caller
                    .as_ref()
                    .is_some_and(|scopes| exposed && converted(scopes).permits(mapped.permission));
                if old == new {
                    continue;
                }
                let scopes = caller.as_ref().expect("unresolved callers never differ");
                match classify(row.scope, mapped.permission, scopes, new) {
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
    let old = scope_baseline();
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

/// The switch put the mapping in force: every resource requires, marks and exposes exactly what
/// the reviewed mapping gives it.
#[test]
fn the_resource_table_holds_the_mapping() {
    let mapped = mapping();
    let table = current();
    assert_eq!(
        table.keys().collect::<BTreeSet<_>>(),
        mapped.keys().collect::<BTreeSet<_>>()
    );
    for (key, row) in &table {
        let expected = &mapped[key];
        assert_eq!(row.permission, expected.permission, "{key:?}");
        assert_eq!(row.sensitive, expected.sensitive, "{key:?}");
        assert_eq!(row.targets, expected.targets, "{key:?}");
    }
    // A resource that moved into the namespace tree sits where the mapping placed it.
    for resource in nyanpasu_rpc_protocol::permission::RESOURCES {
        if resource.source == nyanpasu_rpc_protocol::permission::PermissionSource::Legacy {
            continue;
        }
        let expected = mapped
            .iter()
            .find(|((_, name), _)| name == resource.name)
            .map(|(_, mapped)| mapped.namespace.as_str());
        assert_eq!(Some(resource.namespace), expected, "{}", resource.name);
    }
}

#[test]
fn mapping_covers_the_baseline_with_its_exposure() {
    let before = scope_baseline();
    let after = mapping();
    let mapped: BTreeSet<_> = after.keys().collect();
    assert_eq!(mapped, before.keys().collect());
    for (key, row) in &before {
        assert_eq!(after[key].targets, row.targets, "{key:?}");
    }
}

#[test]
fn requirements_follow_the_reference_derivation() {
    let before = scope_baseline();
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
    let before = scope_baseline();
    for (key, mapped) in mapping() {
        let name = &key.1;
        let was_sensitive = before[&key].scope == "sensitive_read";
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
        &BTreeSet::from(["settings"]),
        true,
    );
    assert_eq!(changed, None);
}

/// The test-only conversion of design section 7, on the permission model's own types.
#[test]
fn legacy_scopes_convert_to_the_design_permissions() {
    use Access::Write;
    assert_eq!(
        converted(&BTreeSet::from(["read"])),
        Role::Viewer.permissions()
    );
    assert_eq!(
        converted(&BTreeSet::from(["sensitive_read"])),
        Permissions::NONE
    );
    assert_eq!(
        converted(&BTreeSet::from(["read", "settings"])),
        Role::Viewer
            .permissions()
            .with(Resource::Settings, Write)
            .with(Resource::Logs, Write)
            .with(Resource::Traffic, Write)
    );
    assert_eq!(
        converted(&SCOPES.into_iter().collect()),
        Role::Owner.permissions()
    );
    assert_eq!(
        converted(&BTreeSet::from(["proxy"])).get(Resource::Proxy),
        Some(Write)
    );
}
