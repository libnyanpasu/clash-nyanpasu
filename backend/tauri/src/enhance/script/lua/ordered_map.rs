//! An insertion-ordered map for Lua scripts.
//!
//! Lua tables do not keep key order, but mihomo reads some mappings in
//! order (DNS `nameserver-policy`, for one). A YAML mapping therefore
//! enters Lua as an OrderedMap: an empty proxy table whose shared
//! metatable forwards reads, writes and `pairs` to an `IndexMap` kept on
//! the Rust side. The proxy is still a table, so `type(map) == "table"`
//! holds and scripts index, assign and iterate it like any other table.
//! `rawget`, `rawset` and `rawlen` bypass the metatable and see an empty
//! table.

use std::{cell::Cell, ffi::c_void};

use indexmap::IndexMap;
use mlua::prelude::*;
use serde_yaml::{Mapping, Value as Yaml};

const METATABLE: &str = "nyanpasu.ordered_map.metatable";
const STORES: &str = "nyanpasu.ordered_map.stores";

/// A map key. Lua only allows these types as keys that YAML can carry.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Key {
    Boolean(bool),
    Integer(i64),
    /// The bits of a float that is not integral. Lua stores integral float
    /// keys as integers, so `map[1.0]` and `map[1]` are the same entry.
    Number(u64),
    String(LuaString),
}

impl Key {
    /// Converts a key for a lookup. `None` means no entry can have it.
    fn lookup(value: &LuaValue) -> Option<Self> {
        match value {
            LuaValue::Boolean(b) => Some(Self::Boolean(*b)),
            LuaValue::Integer(i) => Some(Self::Integer(*i)),
            LuaValue::Number(n) if n.is_nan() => None,
            LuaValue::Number(n)
                if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n < i64::MAX as f64 =>
            {
                Some(Self::Integer(*n as i64))
            }
            LuaValue::Number(n) => Some(Self::Number(n.to_bits())),
            LuaValue::String(s) => Some(Self::String(s.clone())),
            _ => None,
        }
    }

    /// Converts a key for a write, with Lua's messages for the keys it rejects.
    fn from_lua(value: &LuaValue) -> LuaResult<Self> {
        Self::lookup(value).ok_or_else(|| match value {
            LuaValue::Nil => LuaError::runtime("index is nil"),
            LuaValue::Number(_) => LuaError::runtime("index is NaN"),
            other => LuaError::runtime(format!(
                "OrderedMap keys must be strings, numbers or booleans, got {}",
                other.type_name()
            )),
        })
    }

    fn to_lua(&self) -> LuaValue {
        match self {
            Self::Boolean(b) => LuaValue::Boolean(*b),
            Self::Integer(i) => LuaValue::Integer(*i),
            Self::Number(bits) => LuaValue::Number(f64::from_bits(*bits)),
            Self::String(s) => LuaValue::String(s.clone()),
        }
    }
}

/// The entries behind one proxy table. Values are never nil: assigning nil
/// removes the entry, as it does for a table.
struct Store(IndexMap<Key, LuaValue>);

impl LuaUserData for Store {}

/// Installs the `OrderedMap` library and makes `next` understand ordered
/// maps. Must run before [`to_lua`] converts anything.
pub fn register(lua: &Lua) -> LuaResult<()> {
    // Weak keys, so a map the script drops can be collected.
    let stores = lua.create_table()?;
    let weak = lua.create_table()?;
    weak.raw_set("__mode", "k")?;
    stores.set_metatable(Some(weak))?;
    lua.set_named_registry_value(STORES, &stores)?;
    lua.set_named_registry_value(METATABLE, create_metatable(lua)?)?;

    let globals = lua.globals();
    globals.set("OrderedMap", create_library(lua)?)?;

    // `pairs` goes through `__pairs`, but `next` reads the raw table, which
    // for a proxy is always empty.
    let raw_next: LuaFunction = globals.get("next")?;
    let next = lua.create_function(move |lua, (table, key): (LuaValue, LuaValue)| {
        if let LuaValue::Table(map) = &table
            && let Some(store) = store_of(lua, map)?
        {
            let store = store.borrow::<Store>()?;
            let index = match &key {
                LuaValue::Nil => 0,
                key => {
                    Key::lookup(key)
                        .and_then(|key| store.0.get_index_of(&key))
                        .ok_or_else(|| LuaError::runtime("invalid key to 'next'"))?
                        + 1
                }
            };
            return match store.0.get_index(index) {
                Some((key, value)) => (key.to_lua(), value.clone()).into_lua_multi(lua),
                None => LuaValue::Nil.into_lua_multi(lua),
            };
        }
        raw_next.call::<LuaMultiValue>((table, key))
    })?;
    globals.set("next", next)?;
    Ok(())
}

fn create_metatable(lua: &Lua) -> LuaResult<LuaTable> {
    let metatable = lua.create_table()?;
    // Hides the metatable from scripts and stops `setmetatable` replacing it.
    metatable.raw_set("__metatable", "OrderedMap")?;
    metatable.raw_set(
        "__index",
        lua.create_function(|lua, (map, key): (LuaTable, LuaValue)| {
            let Some(key) = Key::lookup(&key) else {
                return Ok(LuaValue::Nil);
            };
            let store = expect_store(lua, &map)?;
            let value = store.borrow::<Store>()?.0.get(&key).cloned();
            Ok(value.unwrap_or(LuaValue::Nil))
        })?,
    )?;
    metatable.raw_set(
        "__newindex",
        lua.create_function(|lua, (map, key, value): (LuaTable, LuaValue, LuaValue)| {
            let key = Key::from_lua(&key)?;
            let store = expect_store(lua, &map)?;
            let mut store = store.borrow_mut::<Store>()?;
            if value.is_nil() {
                store.0.shift_remove(&key);
            } else {
                // An existing key keeps its position; a new one goes last.
                store.0.insert(key, value);
            }
            Ok(())
        })?,
    )?;
    metatable.raw_set(
        "__len",
        lua.create_function(|lua, map: LuaTable| {
            Ok(expect_store(lua, &map)?.borrow::<Store>()?.0.len())
        })?,
    )?;
    metatable.raw_set(
        "__pairs",
        lua.create_function(|lua, map: LuaTable| {
            let store = expect_store(lua, &map)?;
            // Iterate over the keys as they were when the loop started, so
            // assigning nil to the current key mid-loop skips nothing, as
            // it is allowed to for a table.
            let keys: Vec<Key> = store.borrow::<Store>()?.0.keys().cloned().collect();
            let position = Cell::new(0);
            let iterator = lua.create_function(move |_, _: LuaMultiValue| {
                let store = store.borrow::<Store>()?;
                while let Some(key) = keys.get(position.get()) {
                    position.set(position.get() + 1);
                    if let Some(value) = store.0.get(key) {
                        return Ok((key.to_lua(), value.clone()));
                    }
                }
                Ok((LuaValue::Nil, LuaValue::Nil))
            })?;
            Ok((iterator, map, LuaValue::Nil))
        })?,
    )?;
    Ok(metatable)
}

fn create_library(lua: &Lua) -> LuaResult<LuaTable> {
    let library = lua.create_table()?;
    library.set(
        "new",
        lua.create_function(|lua, entries: Option<LuaTable>| {
            let mut map = IndexMap::new();
            for (index, entry) in entries
                .iter()
                .flat_map(|t| t.sequence_values::<LuaValue>())
                .enumerate()
            {
                let entry: LuaValue = entry?;
                let LuaValue::Table(entry) = entry else {
                    return Err(LuaError::runtime(format!(
                        "OrderedMap.new: entry {} must be a {{key, value}} pair",
                        index + 1
                    )));
                };
                let value: LuaValue = entry.raw_get(2)?;
                if !value.is_nil() {
                    map.insert(Key::from_lua(&entry.raw_get(1)?)?, value);
                }
            }
            new_map(lua, map)
        })?,
    )?;
    library.set(
        "is",
        lua.create_function(|lua, value: LuaValue| match value {
            LuaValue::Table(table) => Ok(store_of(lua, &table)?.is_some()),
            _ => Ok(false),
        })?,
    )?;
    library.set(
        "insert",
        lua.create_function(
            |lua, (map, position, key, value): (LuaValue, i64, LuaValue, LuaValue)| {
                let store = library_store(lua, &map, "insert")?;
                let mut store = store.borrow_mut::<Store>()?;
                if value.is_nil() {
                    return Err(LuaError::runtime(
                        "OrderedMap.insert: value is nil, use OrderedMap.remove",
                    ));
                }
                let key = Key::from_lua(&key)?;
                // The key moves if it is already there, so it does not count.
                let len = store.0.len() - usize::from(store.0.contains_key(&key));
                let index = check_position(position, len + 1, "insert")?;
                store.0.shift_remove(&key);
                store.0.shift_insert(index, key, value);
                Ok(())
            },
        )?,
    )?;
    library.set(
        "move",
        lua.create_function(|lua, (map, key, position): (LuaValue, LuaValue, i64)| {
            let store = library_store(lua, &map, "move")?;
            let mut store = store.borrow_mut::<Store>()?;
            let from = Key::lookup(&key)
                .and_then(|key| store.0.get_index_of(&key))
                .ok_or_else(|| LuaError::runtime("OrderedMap.move: key not found"))?;
            let to = check_position(position, store.0.len(), "move")?;
            store.0.move_index(from, to);
            Ok(())
        })?,
    )?;
    library.set(
        "remove",
        lua.create_function(|lua, (map, key): (LuaValue, LuaValue)| {
            let store = library_store(lua, &map, "remove")?;
            let Some(key) = Key::lookup(&key) else {
                return Ok(LuaValue::Nil);
            };
            let removed = store.borrow_mut::<Store>()?.0.shift_remove(&key);
            Ok(removed.unwrap_or(LuaValue::Nil))
        })?,
    )?;
    library.set(
        "index_of",
        lua.create_function(|lua, (map, key): (LuaValue, LuaValue)| {
            let store = library_store(lua, &map, "index_of")?;
            let store = store.borrow::<Store>()?;
            Ok(Key::lookup(&key)
                .and_then(|key| store.0.get_index_of(&key))
                .map(|index| index + 1))
        })?,
    )?;
    library.set(
        "keys",
        lua.create_function(|lua, map: LuaValue| {
            let store = library_store(lua, &map, "keys")?;
            let keys: Vec<LuaValue> = store.borrow::<Store>()?.0.keys().map(Key::to_lua).collect();
            lua.create_sequence_from(keys)
        })?,
    )?;
    library.set(
        "values",
        lua.create_function(|lua, map: LuaValue| {
            let store = library_store(lua, &map, "values")?;
            let values: Vec<LuaValue> = store.borrow::<Store>()?.0.values().cloned().collect();
            lua.create_sequence_from(values)
        })?,
    )?;
    Ok(library)
}

/// Checks a 1-based position against `1..=max` and returns it 0-based.
fn check_position(position: i64, max: usize, function: &str) -> LuaResult<usize> {
    usize::try_from(position)
        .ok()
        .filter(|position| (1..=max).contains(position))
        .map(|position| position - 1)
        .ok_or_else(|| {
            LuaError::runtime(format!(
                "OrderedMap.{function}: position {position} is out of range 1..{max}"
            ))
        })
}

fn new_map(lua: &Lua, entries: IndexMap<Key, LuaValue>) -> LuaResult<LuaTable> {
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(lua.named_registry_value(METATABLE)?))?;
    let stores: LuaTable = lua.named_registry_value(STORES)?;
    stores.raw_set(&proxy, lua.create_userdata(Store(entries))?)?;
    Ok(proxy)
}

/// The store behind `table`, or `None` when it is not an ordered map.
fn store_of(lua: &Lua, table: &LuaTable) -> LuaResult<Option<LuaAnyUserData>> {
    lua.named_registry_value::<LuaTable>(STORES)?.raw_get(table)
}

/// The store behind a table that carries the ordered map metatable.
fn expect_store(lua: &Lua, map: &LuaTable) -> LuaResult<LuaAnyUserData> {
    store_of(lua, map)?.ok_or_else(|| LuaError::runtime("not an OrderedMap"))
}

fn library_store(lua: &Lua, map: &LuaValue, function: &str) -> LuaResult<LuaAnyUserData> {
    let store = match map {
        LuaValue::Table(table) => store_of(lua, table)?,
        _ => None,
    };
    store.ok_or_else(|| {
        LuaError::runtime(format!(
            "bad argument #1 to 'OrderedMap.{function}' (OrderedMap expected, got {})",
            map.type_name()
        ))
    })
}

/// Converts YAML into Lua. Mappings become ordered maps, sequences become
/// arrays.
pub fn to_lua(lua: &Lua, value: &Yaml) -> LuaResult<LuaValue> {
    match value {
        Yaml::Mapping(mapping) => {
            let mut entries = IndexMap::with_capacity(mapping.len());
            for (key, value) in mapping {
                entries.insert(Key::from_lua(&to_lua(lua, key)?)?, to_lua(lua, value)?);
            }
            Ok(LuaValue::Table(new_map(lua, entries)?))
        }
        Yaml::Sequence(sequence) => {
            let table = lua.create_table_with_capacity(sequence.len(), 0)?;
            for value in sequence {
                table.raw_push(to_lua(lua, value)?)?;
            }
            // Keeps an empty sequence a sequence on the way back.
            table.set_metatable(Some(lua.array_metatable()))?;
            Ok(LuaValue::Table(table))
        }
        scalar => lua.to_value(scalar),
    }
}

/// Converts a Lua value back into YAML. Ordered maps keep their order. A
/// plain table has no order of its own, so its keys come out sorted and
/// the output does not change from run to run.
pub fn from_lua(lua: &Lua, value: LuaValue) -> LuaResult<Yaml> {
    convert(lua, value, &mut Vec::new())
}

fn convert(lua: &Lua, value: LuaValue, visiting: &mut Vec<*const c_void>) -> LuaResult<Yaml> {
    let LuaValue::Table(table) = value else {
        return lua.from_value(value);
    };
    let pointer = table.to_pointer();
    if visiting.contains(&pointer) {
        return Err(LuaError::runtime("cannot convert a recursive table"));
    }
    visiting.push(pointer);
    let result = convert_table(lua, &table, visiting);
    visiting.pop();
    result
}

fn convert_table(
    lua: &Lua,
    table: &LuaTable,
    visiting: &mut Vec<*const c_void>,
) -> LuaResult<Yaml> {
    let entries: Vec<(Key, LuaValue)> = if let Some(store) = store_of(lua, table)? {
        // Copy out so the borrow ends before converting nested values.
        let store = store.borrow::<Store>()?;
        store
            .0
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    } else {
        // The same rule mlua's own conversion uses to tell arrays from maps.
        let is_array = table
            .metatable()
            .is_some_and(|metatable| metatable.to_pointer() == lua.array_metatable().to_pointer());
        let len = table.raw_len();
        if is_array || len > 0 {
            let mut sequence = Vec::with_capacity(len);
            for index in 1..=len {
                sequence.push(convert(lua, table.raw_get(index)?, visiting)?);
            }
            return Ok(Yaml::Sequence(sequence));
        }
        let mut entries = table
            .pairs::<LuaValue, LuaValue>()
            .map(|pair| pair.and_then(|(key, value)| Ok((Key::from_lua(&key)?, value))))
            .collect::<LuaResult<Vec<_>>>()?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    };
    let mut mapping = Mapping::with_capacity(entries.len());
    for (key, value) in entries {
        mapping.insert(
            convert(lua, key.to_lua(), visiting)?,
            convert(lua, value, visiting)?,
        );
    }
    Ok(Yaml::Mapping(mapping))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhance::script::create_lua_context;

    /// Runs `script` with `config` loaded from YAML and returns what it
    /// returns, as YAML text.
    fn run(config: &str, script: &str) -> Result<String, String> {
        let lua = create_lua_context().unwrap();
        let config: Yaml = serde_yaml::from_str(config).unwrap();
        lua.globals()
            .set("config", to_lua(&lua, &config).unwrap())
            .unwrap();
        let output = lua
            .load(script)
            .eval::<LuaValue>()
            .map_err(|e| e.to_string())?;
        let output = from_lua(&lua, output).map_err(|e| e.to_string())?;
        Ok(serde_yaml::to_string(&output).unwrap())
    }

    const CONFIG: &str = "zeta: 1\nalpha: 2\nmid: 3\nbeta: 4\ndns:\n  nameserver-policy:\n    p1: a\n    p2: b\n    p3: c\n";

    #[test]
    fn config_keeps_its_key_order() {
        let script = r#"
            config.dns["nameserver-policy"].p0 = "z"
            config.newkey = true
            config.alpha = 20
            config.mid = nil
            return config
        "#;
        // Lua's hash order changes between states, so one lucky run proves little.
        for _ in 0..10 {
            assert_eq!(
                run(CONFIG, script).unwrap(),
                "zeta: 1\nalpha: 20\nbeta: 4\ndns:\n  nameserver-policy:\n    p1: a\n    p2: b\n    p3: c\n    p0: z\nnewkey: true\n"
            );
        }
    }

    #[test]
    fn pairs_and_next_follow_insertion_order() {
        let script = r#"
            local by_pairs, by_next = {}, {}
            for key in pairs(config) do by_pairs[#by_pairs + 1] = key end
            local key = next(config)
            while key ~= nil do
                by_next[#by_next + 1] = key
                key = next(config, key)
            end
            return { table.concat(by_pairs, ","), table.concat(by_next, ",") }
        "#;
        assert_eq!(
            run(CONFIG, script).unwrap(),
            "- zeta,alpha,mid,beta,dns\n- zeta,alpha,mid,beta,dns\n"
        );
    }

    #[test]
    fn clearing_keys_inside_pairs_visits_every_entry() {
        let script = r#"
            local visited = 0
            for key in pairs(config) do
                visited = visited + 1
                config[key] = nil
            end
            return { visited = visited, left = #config }
        "#;
        assert_eq!(run(CONFIG, script).unwrap(), "left: 0\nvisited: 5\n");
    }

    #[test]
    fn ordered_maps_behave_like_tables() {
        let script = r#"
            local empty = OrderedMap.new()
            config[1.0] = "float key"
            return {
                type = type(config),
                len = #config,
                metatable = getmetatable(config),
                empty_next = next(empty) == nil,
                integer_key = config[1],
                missing = config.missing == nil,
                table_key = config[{}] == nil,
                can_replace_metatable = pcall(setmetatable, config, {}),
            }
        "#;
        assert_eq!(
            run(CONFIG, script).unwrap(),
            "can_replace_metatable: false\nempty_next: true\ninteger_key: float key\nlen: 6\nmetatable: OrderedMap\nmissing: true\ntable_key: true\ntype: table\n"
        );
    }

    #[test]
    fn library_places_entries() {
        let script = r#"
            local map = OrderedMap.new({ { "a", 1 }, { "b", 2 } })
            OrderedMap.insert(map, 1, "z", 0)
            local first = table.concat(OrderedMap.keys(map), ",")
            OrderedMap.insert(map, 3, "z", 9)
            OrderedMap.move(map, "b", 1)
            local removed = OrderedMap.remove(map, "a")
            return {
                first = first,
                keys = table.concat(OrderedMap.keys(map), ","),
                values = table.concat(OrderedMap.values(map), ","),
                removed = removed,
                missing = OrderedMap.remove(map, "a") == nil,
                index_of_z = OrderedMap.index_of(map, "z"),
                index_of_missing = OrderedMap.index_of(map, "a") == nil,
                is_map = OrderedMap.is(map),
                is_table = OrderedMap.is({}),
            }
        "#;
        assert_eq!(
            run("{}", script).unwrap(),
            "first: z,a,b\nindex_of_missing: true\nindex_of_z: 2\nis_map: true\nis_table: false\nkeys: b,z\nmissing: true\nremoved: 1\nvalues: 2,9\n"
        );
    }

    #[test]
    fn library_rejects_bad_arguments() {
        let error = |script: &str| run("{}", script).unwrap_err();
        let map = "local map = OrderedMap.new({ { 'a', 1 } })\n";
        assert!(
            error(&format!("{map}OrderedMap.insert(map, 3, 'b', 2)"))
                .contains("OrderedMap.insert: position 3 is out of range 1..2")
        );
        // Moving an existing key frees its own slot, so 2 is out of range too.
        assert!(
            error(&format!("{map}OrderedMap.insert(map, 2, 'a', 2)"))
                .contains("position 2 is out of range 1..1")
        );
        assert!(
            error(&format!("{map}OrderedMap.move(map, 'a', 0)"))
                .contains("OrderedMap.move: position 0 is out of range 1..1")
        );
        assert!(
            error(&format!("{map}OrderedMap.move(map, 'b', 1)"))
                .contains("OrderedMap.move: key not found")
        );
        assert!(
            error(&format!("{map}OrderedMap.insert(map, 1, 'b', nil)"))
                .contains("value is nil, use OrderedMap.remove")
        );
        assert!(
            error("OrderedMap.keys({})")
                .contains("bad argument #1 to 'OrderedMap.keys' (OrderedMap expected, got table)")
        );
        assert!(error("OrderedMap.new({ 1 })").contains("entry 1 must be a {key, value} pair"));
        assert!(error("config[{}] = 1").contains("got table"));
    }

    #[test]
    fn plain_tables_come_out_with_sorted_keys() {
        let script = r#"
            config.dns = { ipv6 = false, enable = true, listen = "0.0.0.0:53" }
            return config
        "#;
        assert_eq!(
            run("a: 1\ndns: {}\n", script).unwrap(),
            "a: 1\ndns:\n  enable: true\n  ipv6: false\n  listen: 0.0.0.0:53\n"
        );
    }

    #[test]
    fn sequences_round_trip() {
        let script = r#"
            table.insert(config.proxies, { name = "new" })
            return config
        "#;
        assert_eq!(
            run("proxies:\n- name: old\n  type: ss\nrules: []\n", script).unwrap(),
            "proxies:\n- name: old\n  type: ss\n- name: new\nrules: []\n"
        );
    }

    #[test]
    fn recursive_tables_are_rejected() {
        let error = run("{}", "config.self = config\nreturn config").unwrap_err();
        assert!(
            error.contains("cannot convert a recursive table"),
            "{error}"
        );
    }
}
