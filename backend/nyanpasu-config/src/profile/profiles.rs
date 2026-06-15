use std::{fmt, str::FromStr};

use indexmap::{IndexMap, map::Entry};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Error, SeqAccess, Visitor},
};
use specta::Type;

use super::item::{ProfileItem, ProfileSource, kind::ProfileId};

/// Defines the top-level `profiles.yaml` schema.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Profiles {
    /// Same as legacy `PrfConfig.current`.
    #[serde(default, deserialize_with = "deserialize_single_or_vec")]
    pub current: Vec<ProfileId>,

    /// Same as legacy `PrfConfig.chain`.
    #[serde(default)]
    pub chain: Vec<ProfileId>,

    /// Clash fields considered valid when extracting runtime config.
    #[serde(default = "default_valid")]
    pub valid: Vec<String>,

    /// Profile list keyed by uid while preserving the original sequence order.
    ///
    /// Invariant: each map key must equal its value's `ProfileItem::meta.uid`.
    /// Prefer the mutation helpers below, which uphold that invariant.
    #[serde(default, with = "items_serde")]
    #[specta(type = Vec<ProfileItem>)]
    pub items: IndexMap<ProfileId, ProfileItem>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            current: Vec::new(),
            chain: Vec::new(),
            valid: default_valid(),
            items: IndexMap::new(),
        }
    }
}

impl Profiles {
    pub fn get_item(&self, uid: &ProfileId) -> Option<&ProfileItem> {
        self.items.get(uid)
    }

    /// Append a new item. Returns `false` (and keeps the existing item) when the
    /// uid already exists.
    pub fn append_item(&mut self, item: ProfileItem) -> bool {
        match self.items.entry(item.meta.uid.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(item);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    /// Replace an existing item in place, returning the previous value. Returns
    /// `None` when the uid is unknown.
    pub fn replace_item(&mut self, item: ProfileItem) -> Option<ProfileItem> {
        self.items
            .get_mut(&item.meta.uid)
            .map(|slot| std::mem::replace(slot, item))
    }

    /// Remove an item, preserving the order of the remaining items.
    pub fn remove_item(&mut self, uid: &ProfileId) -> Option<ProfileItem> {
        self.items.shift_remove(uid)
    }

    /// Move `active_id` to the position of `over_id`. No-op (returns `false`)
    /// when either uid is missing or both refer to the same slot.
    pub fn reorder(&mut self, active_id: &ProfileId, over_id: &ProfileId) -> bool {
        let (Some(active_index), Some(over_index)) = (
            self.items.get_index_of(active_id),
            self.items.get_index_of(over_id),
        ) else {
            return false;
        };

        if active_index == over_index {
            return false;
        }

        self.items.move_index(active_index, over_index);
        true
    }

    /// Reorder items to match `order`. Unknown/duplicate uids are ignored; items
    /// not present in `order` keep their relative order and are appended last.
    pub fn reorder_by_list<I>(&mut self, order: I)
    where
        I: IntoIterator<Item = ProfileId>,
    {
        let mut remaining = std::mem::take(&mut self.items);
        let mut ordered = IndexMap::with_capacity(remaining.len());

        for uid in order {
            if let Some((key, item)) = remaining.shift_remove_entry(&uid) {
                ordered.insert(key, item);
            }
        }

        ordered.extend(remaining);
        self.items = ordered;
    }

    /// Drop dangling `current`/`chain` references (uids no longer present in
    /// `items`) and report the first activatable item.
    ///
    /// This only mutates the in-memory top-level references; file deletion and
    /// the actual activation decision remain the service layer's responsibility.
    pub fn sanitize_current(&mut self) -> ProfilesSanitizeReport {
        let removed_current = retain_existing_refs(&self.items, &mut self.current);
        let removed_chain = retain_existing_refs(&self.items, &mut self.chain);
        let default_activatable = self
            .items
            .iter()
            .find_map(|(uid, item)| match &item.source {
                ProfileSource::Local(_) | ProfileSource::Remote(_) => Some(uid.clone()),
                ProfileSource::Merge(_) | ProfileSource::Script(_) => None,
            });
        let current_needs_activation = self.current.is_empty() && default_activatable.is_some();

        ProfilesSanitizeReport {
            removed_current,
            removed_chain,
            default_activatable,
            current_needs_activation,
        }
    }
}

/// Outcome of [`Profiles::sanitize_current`], handed to the service layer so it
/// can decide whether to persist changes or activate a default profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProfilesSanitizeReport {
    pub removed_current: Vec<ProfileId>,
    pub removed_chain: Vec<ProfileId>,
    pub default_activatable: Option<ProfileId>,
    pub current_needs_activation: bool,
}

fn default_valid() -> Vec<String> {
    vec![
        "dns".into(),
        "unified-delay".into(),
        "tcp-concurrent".into(),
    ]
}

fn retain_existing_refs(
    items: &IndexMap<ProfileId, ProfileItem>,
    refs: &mut Vec<ProfileId>,
) -> Vec<ProfileId> {
    let mut removed = Vec::new();
    refs.retain(|uid| {
        let keep = items.contains_key(uid);
        if !keep {
            removed.push(uid.clone());
        }
        keep
    });
    removed
}

/// Decode either a single bare string or a sequence of strings into a `Vec<T>`,
/// matching the legacy `current` wire format.
fn deserialize_single_or_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    struct StringOrVec<T>(std::marker::PhantomData<T>);

    impl<'de, T> Visitor<'de> for StringOrVec<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or a sequence of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            T::from_str(value)
                .map(|value| vec![value])
                .map_err(E::custom)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element::<String>()? {
                values.push(T::from_str(&value).map_err(A::Error::custom)?);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_any(StringOrVec(std::marker::PhantomData))
}

/// Serde glue that keeps the on-disk `items` shape a YAML sequence
/// (`items: [{uid, ...}]`) while the in-memory model is an [`IndexMap`] keyed by
/// uid. Duplicate uids keep the first occurrence and are logged.
mod items_serde {
    use super::*;

    pub fn serialize<S>(
        items: &IndexMap<ProfileId, ProfileItem>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(items.values())
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<IndexMap<ProfileId, ProfileItem>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let items = Vec::<ProfileItem>::deserialize(deserializer)?;
        let mut map = IndexMap::with_capacity(items.len());
        let mut duplicates = Vec::new();

        for item in items {
            match map.entry(item.meta.uid.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(item);
                }
                Entry::Occupied(entry) => duplicates.push(entry.key().clone()),
            }
        }

        if !duplicates.is_empty() {
            tracing::warn!(?duplicates, "duplicate profile ids ignored");
        }

        Ok(map)
    }
}
