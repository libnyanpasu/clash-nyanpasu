//! Projecting a configuration document to its portable part, and back.
//!
//! Projection drops every device-bound field. Applying is the inverse: the
//! device's current document supplies the device-bound fields and the
//! projection supplies the portable ones, so a merged projection can replace a
//! document without disturbing the machine it lands on.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use snafu::{ResultExt as _, Snafu, ensure};

use super::{
    canonical::canonical_json,
    scope::{APPLICATION_SCOPES, CLASH_SCOPES, DocumentKind, FieldScope, scope_of},
};
use crate::{
    application::NyanpasuAppConfig,
    clash::config::ClashConfig,
    profile::{ConfigDefinition, ProfileDefinition, ProfileItem, Profiles, TransformDefinition},
};

/// The portable revision this build writes and the highest it can read.
///
/// Additive schema changes do not bump it: serde defaults absorb them. A
/// breaking change bumps it and adds an upgrade from the previous revision.
pub const PORTABLE_REVISION: u32 = 1;

/// Payload keys of the profiles projection.
pub(super) const ITEMS: &str = "items";
pub(super) const ITEM_ORDER: &str = "order";
pub(super) const FOLDERS: &str = "folders";
pub(super) const FOLDER_ORDER: &str = "folder_order";
pub(super) const OVERRIDES: &str = "overrides";
const PROXY_GROUP_LAYOUTS: &str = "proxy_group_layouts";
const GLOBAL_TRANSFORMS: &str = "global_transforms";
const VALID: &str = "valid";

/// Where a profile item's source sits in its serialized form.
const SOURCE_POINTERS: [&str; 2] = ["/config/source", "/transform/source"];

/// The portable part of one configuration document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableDocument {
    pub document: DocumentKind,
    pub portable_revision: u32,
    pub payload: Map<String, Value>,
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
pub enum DocumentError {
    #[snafu(display("portable document is not valid JSON"))]
    Decode { source: serde_json::Error },
    #[snafu(display("expected a {expected:?} projection, found {found:?}"))]
    WrongKind {
        expected: DocumentKind,
        found: DocumentKind,
    },
    #[snafu(display("portable revision {remote} is newer than this build supports ({supported})"))]
    SchemaTooNew { remote: u32, supported: u32 },
    #[snafu(display("failed to serialize the {kind:?} document"))]
    Encode {
        kind: DocumentKind,
        source: serde_json::Error,
    },
    #[snafu(display("the merged {kind:?} projection does not form a valid document"))]
    Rebuild {
        kind: DocumentKind,
        source: serde_json::Error,
    },
    #[snafu(display("the merged catalog no longer contains the active profile {uid}"))]
    ActiveProfileMissing { uid: String },
}

impl PortableDocument {
    /// Decodes a projection read from the remote or the baseline.
    ///
    /// # Errors
    ///
    /// [`DocumentError::SchemaTooNew`] when a newer build wrote it; the caller
    /// must neither apply it nor overwrite it.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DocumentError> {
        let document: Self = serde_json::from_slice(bytes).context(DecodeSnafu)?;
        ensure!(
            document.portable_revision <= PORTABLE_REVISION,
            SchemaTooNewSnafu {
                remote: document.portable_revision,
                supported: PORTABLE_REVISION,
            }
        );
        Ok(document)
    }

    /// The bytes that are hashed, uploaded and stored as a merge base.
    #[must_use]
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        let value = serde_json::json!({
            "document": self.document,
            "portable_revision": self.portable_revision,
            "payload": self.payload,
        });
        canonical_json(&value)
    }

    /// The content id of [`PortableDocument::to_canonical_bytes`].
    #[must_use]
    pub fn object_id(&self) -> super::canonical::ObjectId {
        super::canonical::ObjectId::of(&self.to_canonical_bytes())
    }

    fn new(document: DocumentKind, payload: Map<String, Value>) -> Self {
        Self {
            document,
            portable_revision: PORTABLE_REVISION,
            payload,
        }
    }

    fn expect_kind(&self, expected: DocumentKind) -> Result<(), DocumentError> {
        ensure!(
            self.document == expected,
            WrongKindSnafu {
                expected,
                found: self.document,
            }
        );
        Ok(())
    }
}

fn to_object<T: Serialize>(
    kind: DocumentKind,
    document: &T,
) -> Result<Map<String, Value>, DocumentError> {
    match serde_json::to_value(document).context(EncodeSnafu { kind })? {
        Value::Object(map) => Ok(map),
        _ => unreachable!("configuration documents are structs and serialize as mappings"),
    }
}

/// The portable fields of `application.yaml`.
///
/// # Errors
///
/// Fails only if the document cannot be serialized.
pub fn project_application(config: &NyanpasuAppConfig) -> Result<PortableDocument, DocumentError> {
    let kind = DocumentKind::Application;
    let mut payload = to_object(kind, config)?;
    payload.retain(|key, _| scope_of(APPLICATION_SCOPES, key) == Some(FieldScope::Portable));
    Ok(PortableDocument::new(kind, payload))
}

/// `current` with its portable fields replaced by `merged`.
///
/// Fields of `merged` this build does not know are ignored. A portable field
/// absent from `merged` falls back to its serde default.
///
/// # Errors
///
/// [`DocumentError::Rebuild`] when the result does not deserialize.
pub fn apply_application(
    current: &NyanpasuAppConfig,
    merged: &PortableDocument,
) -> Result<NyanpasuAppConfig, DocumentError> {
    let kind = DocumentKind::Application;
    merged.expect_kind(kind)?;
    let mut document = to_object(kind, current)?;
    for (field, scope) in APPLICATION_SCOPES {
        if *scope == FieldScope::Portable {
            replace_field(&mut document, field, merged.payload.get(*field));
        }
    }
    serde_json::from_value(Value::Object(document)).context(RebuildSnafu { kind })
}

/// The portable fields of `clash-config.yaml`; `overrides` keeps only its portable keys.
///
/// # Errors
///
/// Fails only if the document cannot be serialized.
pub fn project_clash(config: &ClashConfig) -> Result<PortableDocument, DocumentError> {
    let kind = DocumentKind::Clash;
    let mut payload = to_object(kind, config)?;
    payload.retain(|key, value| match (key.as_str(), value) {
        (OVERRIDES, Value::Object(overrides)) => {
            overrides.retain(|inner, _| clash_override_scope(inner) == Some(FieldScope::Portable));
            true
        }
        _ => scope_of(CLASH_SCOPES, key) == Some(FieldScope::Portable),
    });
    Ok(PortableDocument::new(kind, payload))
}

/// `current` with its portable fields replaced by `merged`; see [`apply_application`].
///
/// # Errors
///
/// [`DocumentError::Rebuild`] when the result does not deserialize.
pub fn apply_clash(
    current: &ClashConfig,
    merged: &PortableDocument,
) -> Result<ClashConfig, DocumentError> {
    let kind = DocumentKind::Clash;
    merged.expect_kind(kind)?;
    let mut document = to_object(kind, current)?;
    let merged_overrides = merged.payload.get(OVERRIDES).and_then(Value::as_object);
    for (field, scope) in CLASH_SCOPES {
        if *scope != FieldScope::Portable {
            continue;
        }
        match field.strip_prefix("overrides.") {
            Some(inner) => {
                if let Some(Value::Object(overrides)) = document.get_mut(OVERRIDES) {
                    replace_field(
                        overrides,
                        inner,
                        merged_overrides.and_then(|map| map.get(inner)),
                    );
                }
            }
            None => replace_field(&mut document, field, merged.payload.get(*field)),
        }
    }
    serde_json::from_value(Value::Object(document)).context(RebuildSnafu { kind })
}

pub(super) fn clash_override_scope(key: &str) -> Option<FieldScope> {
    scope_of(CLASH_SCOPES, &format!("{OVERRIDES}.{key}"))
}

fn replace_field(document: &mut Map<String, Value>, field: &str, value: Option<&Value>) {
    match value {
        Some(value) => {
            document.insert(field.to_owned(), value.clone());
        }
        None => {
            document.remove(field);
        }
    }
}

/// The portable catalog.
///
/// Items and folders are keyed by id with their order kept beside them, so a
/// merge can treat each as one unit. `revision`, `current`, materialization
/// times and subscription usage stay on the device. An externally bound local
/// profile is emitted as a managed file: other devices receive its content,
/// never its path.
///
/// # Errors
///
/// Fails only if the catalog cannot be serialized.
pub fn project_profiles(profiles: &Profiles) -> Result<PortableDocument, DocumentError> {
    let kind = DocumentKind::Profiles;
    let mut items = Map::new();
    let mut order = Vec::new();
    for (uid, item) in &profiles.items {
        items.insert(uid.0.clone(), project_item(item)?);
        order.push(Value::from(uid.0.as_str()));
    }
    let mut folders = Map::new();
    let mut folder_order = Vec::new();
    for folder in &profiles.folders {
        let value = serde_json::to_value(folder).context(EncodeSnafu { kind })?;
        let id = folder_id(&value);
        folder_order.push(Value::from(id.as_str()));
        folders.insert(id, value);
    }
    let encode = |value: Result<Value, serde_json::Error>| value.context(EncodeSnafu { kind });

    let mut payload = Map::new();
    payload.insert(ITEMS.to_owned(), Value::Object(items));
    payload.insert(ITEM_ORDER.to_owned(), Value::Array(order));
    payload.insert(FOLDERS.to_owned(), Value::Object(folders));
    payload.insert(FOLDER_ORDER.to_owned(), Value::Array(folder_order));
    payload.insert(
        PROXY_GROUP_LAYOUTS.to_owned(),
        encode(serde_json::to_value(&profiles.proxy_group_layouts))?,
    );
    payload.insert(
        GLOBAL_TRANSFORMS.to_owned(),
        encode(serde_json::to_value(&profiles.global_transforms))?,
    );
    payload.insert(
        VALID.to_owned(),
        encode(serde_json::to_value(&profiles.valid))?,
    );
    Ok(PortableDocument::new(kind, payload))
}

fn folder_id(folder: &Value) -> String {
    match folder.get("id") {
        Some(Value::String(id)) => id.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn project_item(item: &ProfileItem) -> Result<Value, DocumentError> {
    let kind = DocumentKind::Profiles;
    let mut value = serde_json::to_value(item).context(EncodeSnafu { kind })?;
    for pointer in SOURCE_POINTERS {
        if let Some(Value::Object(source)) = value.pointer_mut(pointer) {
            strip_device_state(source);
        }
    }
    Ok(value)
}

fn strip_device_state(source: &mut Map<String, Value>) {
    source.remove("updated_at");
    source.remove("subscription");
    if let Some(Value::Object(binding)) = source.get_mut("binding") {
        binding.retain(|key, _| key == "file");
        binding.insert("type".to_owned(), Value::from("managed"));
    }
}

/// The form of a merged catalog item this build would have written itself.
///
/// The merge compares items through it, so a field a newer build added to an
/// item is not read as a local edit by an older one.
pub(super) fn normalize_item(raw: &Value) -> Value {
    serde_json::from_value::<ProfileItem>(raw.clone())
        .ok()
        .and_then(|item| project_item(&item).ok())
        .unwrap_or_else(|| raw.clone())
}

/// `current` with its portable catalog replaced by `merged`.
///
/// Device state is carried over by profile id: the active profile, the durable
/// revision, materialization times, subscription usage and an external
/// binding. An item the device did not have starts with empty device state.
/// The result is a candidate: the profiles owner still validates it.
///
/// # Errors
///
/// [`DocumentError::ActiveProfileMissing`] when the merge would remove the
/// profile this device has active, and [`DocumentError::Rebuild`] when the
/// result does not deserialize.
pub fn apply_profiles(
    current: &Profiles,
    merged: &PortableDocument,
) -> Result<Profiles, DocumentError> {
    let kind = DocumentKind::Profiles;
    merged.expect_kind(kind)?;
    let mut document = to_object(kind, current)?;
    let empty = Map::new();
    let section = |name: &str| super::merge::as_object(merged.payload.get(name), &empty);

    let merged_items = section(ITEMS);
    if let Some(active) = &current.current {
        ensure!(
            merged_items.contains_key(&active.0),
            ActiveProfileMissingSnafu { uid: &active.0 }
        );
    }
    let mut items = Vec::new();
    for uid in ordered_keys(merged_items, merged.payload.get(ITEM_ORDER)) {
        let mut item = merged_items[&uid].clone();
        if let Some(existing) = current.items.iter().find(|(id, _)| id.0 == uid) {
            let existing = serde_json::to_value(existing.1).context(EncodeSnafu { kind })?;
            restore_device_state(&mut item, &existing);
        }
        items.push(item);
    }
    document.insert(ITEMS.to_owned(), Value::Array(items));

    let merged_folders = section(FOLDERS);
    let folders = ordered_keys(merged_folders, merged.payload.get(FOLDER_ORDER))
        .into_iter()
        .map(|id| merged_folders[&id].clone())
        .collect();
    document.insert(FOLDERS.to_owned(), Value::Array(folders));
    for field in [PROXY_GROUP_LAYOUTS, GLOBAL_TRANSFORMS, VALID] {
        replace_field(&mut document, field, merged.payload.get(field));
    }
    serde_json::from_value(Value::Object(document)).context(RebuildSnafu { kind })
}

/// The keys of `map` in the order `order` gives, followed by keys it omits.
fn ordered_keys(map: &Map<String, Value>, order: Option<&Value>) -> Vec<String> {
    let mut keys: Vec<String> = order
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|key| map.contains_key(*key))
        .map(str::to_owned)
        .collect();
    let listed: BTreeSet<String> = keys.iter().cloned().collect();
    let mut missing: Vec<String> = map
        .keys()
        .filter(|key| !listed.contains(*key))
        .cloned()
        .collect();
    missing.sort_unstable();
    keys.extend(missing);
    keys
}

fn restore_device_state(item: &mut Value, existing: &Value) {
    for pointer in SOURCE_POINTERS {
        let (Some(Value::Object(source)), Some(Value::Object(previous))) =
            (item.pointer_mut(pointer), existing.pointer(pointer))
        else {
            continue;
        };
        if source.get("type") != previous.get("type") {
            continue;
        }
        if let Some(subscription) = previous.get("subscription") {
            source.insert("subscription".to_owned(), subscription.clone());
        }
        if let Some(updated_at) = previous.get("updated_at") {
            source.insert("updated_at".to_owned(), updated_at.clone());
        }
        let (Some(Value::Object(binding)), Some(Value::Object(previous_binding))) =
            (source.get_mut("binding"), previous.get("binding"))
        else {
            continue;
        };
        if binding.get("file") == previous_binding.get("file") {
            *binding = previous_binding.clone();
        }
    }
}

/// The managed files whose content is portable: the file of every profile
/// with a local source. A subscription download is fetched again by each
/// device and is never an item.
#[must_use]
pub fn portable_profile_files(profiles: &Profiles) -> BTreeSet<String> {
    profiles
        .items
        .values()
        .filter_map(|item| {
            let source = match &item.definition {
                ProfileDefinition::Config {
                    config: ConfigDefinition::File(file),
                } => &file.source,
                ProfileDefinition::Config {
                    config: ConfigDefinition::Composition(_),
                } => return None,
                ProfileDefinition::Transform {
                    transform: TransformDefinition::Overlay(overlay),
                } => &overlay.source,
                ProfileDefinition::Transform {
                    transform: TransformDefinition::Script(script),
                } => &script.source,
            };
            (!source.is_remote()).then(|| source.materialized().file.as_str().to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn catalog() -> Profiles {
        serde_json::from_value(json!({
            "revision": 7,
            "current": "rRemote",
            "global_transforms": ["sScript"],
            "folders": [{"id": "f1", "name": "Work", "members": ["rRemote"]}],
            "items": [
                {
                    "uid": "rRemote", "name": "Subscription", "type": "config",
                    "config": {"type": "file", "source": {
                        "type": "remote", "file": "rRemote.yaml", "updated_at": 1700000000,
                        "url": "https://example.com/sub", "option": {},
                        "subscription": {"upload": 1, "download": 2, "total": 3}
                    }}
                },
                {
                    "uid": "lLocal", "name": "Bound", "type": "config",
                    "config": {"type": "file", "source": {"type": "local", "binding": {
                        "type": "external", "file": "lLocal.yaml", "updated_at": 1700000001,
                        "target": "/home/user/bound.yaml", "mode": "mirror"
                    }}}
                },
                {
                    "uid": "sScript", "name": "Script", "type": "transform",
                    "transform": {"type": "script", "runtime": "lua", "source": {
                        "type": "local", "binding": {"type": "managed", "file": "sScript.lua"}
                    }}
                }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn application_projection_holds_only_portable_fields_and_round_trips() {
        let config = NyanpasuAppConfig {
            enable_system_proxy: true,
            ..Default::default()
        };
        let projection = project_application(&config).unwrap();
        assert!(projection.payload.contains_key("language"));
        assert!(!projection.payload.contains_key("enable_system_proxy"));
        assert!(!projection.payload.contains_key("hotkeys"));

        let mut remote = projection.clone();
        remote
            .payload
            .insert("traffic_graph".to_owned(), json!(!config.traffic_graph));
        remote
            .payload
            .insert("added_by_a_newer_build".to_owned(), json!(1));
        let applied = apply_application(&config, &remote).unwrap();
        assert_eq!(applied.traffic_graph, !config.traffic_graph);
        assert!(
            applied.enable_system_proxy,
            "device fields come from the device"
        );
    }

    #[test]
    fn clash_projection_never_carries_the_controller_secret() {
        let config = ClashConfig::default();
        let projection = project_clash(&config).unwrap();
        let overrides = projection.payload[OVERRIDES].as_object().unwrap();
        assert!(overrides.contains_key("mode"));
        assert!(!overrides.contains_key("secret"));
        assert!(!projection.payload.contains_key("mixed_port"));

        let mut remote = projection.clone();
        remote.payload[OVERRIDES]["ipv6"] = json!(true);
        let applied = apply_clash(&config, &remote).unwrap();
        assert_eq!(applied.overrides.secret(), config.overrides.secret());
        assert_eq!(project_clash(&applied).unwrap(), remote);
    }

    #[test]
    fn syncing_cannot_replace_the_device_local_editor() {
        use crate::application::ProfileFileOpener;
        let config = NyanpasuAppConfig {
            profile_file_opener: ProfileFileOpener::VsCode,
            ..Default::default()
        };
        let mut remote = project_application(&config).unwrap();
        assert!(!remote.payload.contains_key("profile_file_opener"));
        remote.payload.insert(
            "profile_file_opener".into(),
            json!({"type": "custom", "path": "editor"}),
        );
        assert_eq!(
            apply_application(&config, &remote)
                .unwrap()
                .profile_file_opener,
            ProfileFileOpener::VsCode
        );
    }

    #[test]
    fn profile_projection_drops_device_state() {
        let projection = project_profiles(&catalog()).unwrap();
        let text = String::from_utf8(projection.to_canonical_bytes()).unwrap();
        for device_only in [
            "updated_at",
            "subscription",
            "/home/user",
            "external",
            "\"revision\"",
        ] {
            assert!(!text.contains(device_only), "{device_only} leaked: {text}");
        }
        assert!(!projection.payload.contains_key("current"));
        assert_eq!(
            projection.payload[ITEM_ORDER],
            json!(["rRemote", "lLocal", "sScript"])
        );
        assert_eq!(
            projection.payload[ITEMS]["lLocal"]["config"]["source"]["binding"],
            json!({"type": "managed", "file": "lLocal.yaml"})
        );
        assert_eq!(
            projection.payload[ITEMS]["rRemote"]["config"]["source"]["url"],
            json!("https://example.com/sub")
        );
    }

    #[test]
    fn applying_the_own_projection_changes_nothing() {
        let catalog = catalog();
        let projection = project_profiles(&catalog).unwrap();
        let applied = apply_profiles(&catalog, &projection).unwrap();
        assert_eq!(
            serde_json::to_value(&applied).unwrap(),
            serde_json::to_value(&catalog).unwrap()
        );
    }

    #[test]
    fn applying_a_remote_catalog_keeps_device_state_by_profile_id() {
        let catalog = catalog();
        let mut remote = project_profiles(&catalog).unwrap();
        remote.payload[ITEMS]["rRemote"]["name"] = json!("Renamed");
        remote.payload[ITEMS].as_object_mut().unwrap().insert(
            "nNew".to_owned(),
            json!({"uid": "nNew", "name": "New", "type": "config", "config": {"type": "file",
                "source": {"type": "local", "binding": {"type": "managed", "file": "nNew.yaml"}}}}),
        );
        remote.payload[ITEM_ORDER] = json!(["nNew", "rRemote", "lLocal", "sScript"]);

        let applied = apply_profiles(&catalog, &remote).unwrap();
        let value = serde_json::to_value(&applied).unwrap();
        assert_eq!(applied.revision(), 7);
        assert_eq!(value["current"], json!("rRemote"));
        assert_eq!(value["items"][0]["uid"], json!("nNew"));
        assert_eq!(value["items"][1]["name"], json!("Renamed"));
        assert_eq!(
            value["items"][1]["config"]["source"]["subscription"]["total"],
            json!(3)
        );
        assert_eq!(
            value["items"][1]["config"]["source"]["updated_at"],
            json!(1700000000)
        );
        assert_eq!(
            value["items"][2]["config"]["source"]["binding"]["target"],
            json!("/home/user/bound.yaml")
        );
        assert_eq!(project_profiles(&applied).unwrap(), remote);
    }

    #[test]
    fn a_catalog_without_the_active_profile_is_refused() {
        let catalog = catalog();
        let mut remote = project_profiles(&catalog).unwrap();
        remote.payload[ITEMS]
            .as_object_mut()
            .unwrap()
            .remove("rRemote");
        assert!(matches!(
            apply_profiles(&catalog, &remote),
            Err(DocumentError::ActiveProfileMissing { uid }) if uid == "rRemote"
        ));
    }

    #[test]
    fn only_local_sources_have_portable_files() {
        assert_eq!(
            portable_profile_files(&catalog()),
            BTreeSet::from(["lLocal.yaml".to_owned(), "sScript.lua".to_owned()])
        );
    }

    #[test]
    fn a_newer_revision_is_reported_not_decoded() {
        let bytes = br#"{"document":"clash","portable_revision":2,"payload":{}}"#;
        assert!(matches!(
            PortableDocument::from_bytes(bytes),
            Err(DocumentError::SchemaTooNew {
                remote: 2,
                supported: 1
            })
        ));
    }
}
