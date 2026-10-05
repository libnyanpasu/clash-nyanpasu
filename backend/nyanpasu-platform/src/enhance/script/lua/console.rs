//! The `console` global and `print` for Lua scripts. They follow boa's
//! console, so a JS and a Lua script in the same chain log alike.

use std::{
    cell::RefCell, collections::HashMap, ffi::c_void, fmt::Write as _, rc::Rc, time::Instant,
};

use mlua::prelude::*;
use nyanpasu_config::runtime::executor::StepLogLevel;

use super::ordered_map;
use crate::enhance::script::runner::ConsoleSink;

/// Tables nested deeper than this print as `{...}`.
const MAX_DEPTH: usize = 4;

const KEYWORDS: [&str; 22] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

#[derive(Default)]
struct State {
    groups: usize,
    counts: HashMap<String, u64>,
    timers: HashMap<String, Instant>,
}

#[derive(Clone)]
struct Console {
    sink: ConsoleSink,
    state: Rc<RefCell<State>>,
}

impl Console {
    /// Logs `message`, indented two spaces per open group.
    fn emit(&self, level: StepLogLevel, message: &str) {
        let indent = "  ".repeat(self.state.borrow().groups);
        let message = if indent.is_empty() {
            message.to_string()
        } else {
            message
                .split('\n')
                .map(|line| format!("{indent}{line}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.sink.push(level, message);
    }

    fn add<A: FromLuaMulti>(
        &self,
        lua: &Lua,
        table: &LuaTable,
        name: &str,
        f: impl Fn(&Lua, &Console, A) -> LuaResult<()> + 'static,
    ) -> LuaResult<()> {
        let console = self.clone();
        table.set(
            name,
            lua.create_function(move |lua, args| f(lua, &console, args))?,
        )
    }
}

/// Installs `console` and points `print` at `console.log`. Everything goes
/// to `sink`.
pub fn register(lua: &Lua, sink: &ConsoleSink) -> LuaResult<()> {
    let console = Console {
        sink: sink.clone(),
        state: Rc::default(),
    };
    let table = lua.create_table()?;
    for (name, level) in [
        ("log", StepLogLevel::Log),
        ("info", StepLogLevel::Info),
        ("warn", StepLogLevel::Warn),
        ("error", StepLogLevel::Error),
        ("debug", StepLogLevel::Log),
    ] {
        console.add(
            lua,
            &table,
            name,
            move |lua, console, args: LuaMultiValue| {
                console.emit(level, &format_args(lua, args)?);
                Ok(())
            },
        )?;
    }
    console.add(lua, &table, "trace", |lua, console, args: LuaMultiValue| {
        let message = format_args(lua, args)?;
        // Level 1 starts the traceback at the script frame that called us.
        let traceback = lua.traceback(Some(&message), 1)?;
        console.emit(StepLogLevel::Log, &traceback.to_string_lossy());
        Ok(())
    })?;
    console.add(
        lua,
        &table,
        "assert",
        |lua, console, (condition, args): (LuaValue, LuaMultiValue)| {
            if !matches!(condition, LuaValue::Nil | LuaValue::Boolean(false)) {
                return Ok(());
            }
            let details = format_args(lua, args)?;
            let message = if details.is_empty() {
                "Assertion failed".to_string()
            } else {
                format!("Assertion failed: {details}")
            };
            console.emit(StepLogLevel::Error, &message);
            Ok(())
        },
    )?;
    console.add(lua, &table, "count", |lua, console, label: LuaValue| {
        let label = label_of(lua, label)?;
        let count = {
            let mut state = console.state.borrow_mut();
            let count = state.counts.entry(label.clone()).or_default();
            *count += 1;
            *count
        };
        console.emit(StepLogLevel::Info, &format!("count {label}: {count}"));
        Ok(())
    })?;
    console.add(
        lua,
        &table,
        "countReset",
        |lua, console, label: LuaValue| {
            let label = label_of(lua, label)?;
            let existed = console.state.borrow_mut().counts.remove(&label).is_some();
            if !existed {
                console.emit(
                    StepLogLevel::Warn,
                    &format!("Count for '{label}' does not exist"),
                );
            }
            Ok(())
        },
    )?;
    console.add(lua, &table, "time", |lua, console, label: LuaValue| {
        let label = label_of(lua, label)?;
        let started = {
            let mut state = console.state.borrow_mut();
            let absent = !state.timers.contains_key(&label);
            if absent {
                state.timers.insert(label.clone(), Instant::now());
            }
            absent
        };
        if !started {
            console.emit(
                StepLogLevel::Warn,
                &format!("Timer '{label}' already exists"),
            );
        }
        Ok(())
    })?;
    console.add(
        lua,
        &table,
        "timeLog",
        |lua, console, (label, args): (LuaValue, LuaMultiValue)| {
            let label = label_of(lua, label)?;
            let started = console.state.borrow().timers.get(&label).copied();
            let Some(started) = started else {
                return missing_timer(console, &label);
            };
            let mut message = format!("{label}: {}", elapsed(started));
            let details = format_args(lua, args)?;
            if !details.is_empty() {
                message = format!("{message} {details}");
            }
            console.emit(StepLogLevel::Log, &message);
            Ok(())
        },
    )?;
    console.add(lua, &table, "timeEnd", |lua, console, label: LuaValue| {
        let label = label_of(lua, label)?;
        let started = console.state.borrow_mut().timers.remove(&label);
        let Some(started) = started else {
            return missing_timer(console, &label);
        };
        console.emit(
            StepLogLevel::Info,
            &format!("{label}: {} - timer removed", elapsed(started)),
        );
        Ok(())
    })?;
    for name in ["group", "groupCollapsed"] {
        console.add(lua, &table, name, |lua, console, args: LuaMultiValue| {
            console.emit(
                StepLogLevel::Info,
                &format!("group: {}", format_args(lua, args)?),
            );
            console.state.borrow_mut().groups += 1;
            Ok(())
        })?;
    }
    console.add(lua, &table, "groupEnd", |_, console, ()| {
        let mut state = console.state.borrow_mut();
        state.groups = state.groups.saturating_sub(1);
        Ok(())
    })?;

    let globals = lua.globals();
    // Without this `print` writes to the process's stdout, where nobody sees it.
    globals.set("print", table.get::<LuaFunction>("log")?)?;
    globals.set("console", table)?;
    Ok(())
}

fn missing_timer(console: &Console, label: &str) -> LuaResult<()> {
    console.emit(
        StepLogLevel::Warn,
        &format!("Timer '{label}' does not exist"),
    );
    Ok(())
}

fn elapsed(started: Instant) -> String {
    format!("{:.3} ms", started.elapsed().as_secs_f64() * 1000.0)
}

fn label_of(lua: &Lua, label: LuaValue) -> LuaResult<String> {
    match label {
        LuaValue::Nil => Ok("default".to_string()),
        label => format_args(lua, LuaMultiValue::from_iter([label])),
    }
}

/// Joins the arguments with spaces. Strings print as they are; everything
/// else prints as a Lua value.
fn format_args(lua: &Lua, args: LuaMultiValue) -> LuaResult<String> {
    let mut out = String::new();
    for (index, value) in args.into_iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        match value {
            LuaValue::String(s) => out.push_str(&s.to_string_lossy()),
            value => write_value(lua, &value, 0, &mut Vec::new(), &mut out)?,
        }
    }
    Ok(out)
}

fn write_value(
    lua: &Lua,
    value: &LuaValue,
    depth: usize,
    visiting: &mut Vec<*const c_void>,
    out: &mut String,
) -> LuaResult<()> {
    match value {
        LuaValue::String(s) => {
            let _ = write!(out, "{:?}", s.to_string_lossy());
        }
        // The YAML null a config value can hold.
        LuaValue::LightUserData(ud) if ud.0.is_null() => out.push_str("null"),
        LuaValue::Table(table) => write_table(lua, table, depth, visiting, out)?,
        other => out.push_str(&other.to_string()?),
    }
    Ok(())
}

fn write_table(
    lua: &Lua,
    table: &LuaTable,
    depth: usize,
    visiting: &mut Vec<*const c_void>,
    out: &mut String,
) -> LuaResult<()> {
    let pointer = table.to_pointer();
    if visiting.contains(&pointer) {
        out.push_str("<cycle>");
        return Ok(());
    }
    let entries = ordered_map::entries(lua, table)?;
    if entries.is_empty() {
        out.push_str("{}");
        return Ok(());
    }
    if depth >= MAX_DEPTH {
        out.push_str("{...}");
        return Ok(());
    }
    visiting.push(pointer);
    out.push_str("{ ");
    let mut next_index = 1;
    for (index, (key, value)) in entries.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        match key {
            // Array items print without their index, as in a table constructor.
            LuaValue::Integer(i) if *i == next_index => next_index += 1,
            LuaValue::String(s) if is_identifier(&s.to_string_lossy()) => {
                let _ = write!(out, "{} = ", s.to_string_lossy());
            }
            key => {
                out.push('[');
                write_value(lua, key, depth + 1, visiting, out)?;
                out.push_str("] = ");
            }
        }
        write_value(lua, value, depth + 1, visiting, out)?;
    }
    out.push_str(" }");
    visiting.pop();
    Ok(())
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&name)
}

#[cfg(test)]
mod tests {
    use nyanpasu_config::runtime::executor::{StepLogEntry, StepLogLevel::*};

    use super::*;
    use crate::enhance::script::create_lua_context;

    /// Runs `script` with `config` loaded from YAML and returns what it logged.
    fn logs(config: &str, script: &str) -> Vec<StepLogEntry> {
        let lua = create_lua_context().unwrap();
        let sink = ConsoleSink::default();
        register(&lua, &sink).unwrap();
        let config: serde_yaml::Value = serde_yaml::from_str(config).unwrap();
        lua.globals()
            .set("config", ordered_map::to_lua(&lua, &config).unwrap())
            .unwrap();
        lua.load(script).set_name("=script").exec().unwrap();
        sink.take()
    }

    fn messages(entries: &[StepLogEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.message.as_str()).collect()
    }

    #[test]
    fn logs_every_argument() {
        let logs = logs(
            "{}",
            r#"console.log("text", 1, 2.5, true, nil, { 1, 2, { x = "y" } })"#,
        );
        assert_eq!(
            logs,
            vec![StepLogEntry::new(
                Log,
                r#"text 1 2.5 true nil { 1, 2, { x = "y" } }"#
            )]
        );
    }

    #[test]
    fn prints_ordered_maps_in_order_and_plain_tables_sorted() {
        let logs = logs(
            "zeta: 1\nalpha: ~\nlist: [a]\n",
            r#"
                console.log(config)
                console.log({ b = 1, a = 2, ["with space"] = 3, [10] = 4, ["end"] = 5 })
            "#,
        );
        assert_eq!(
            messages(&logs),
            [
                r#"{ zeta = 1, alpha = null, list = { "a" } }"#,
                r#"{ [10] = 4, a = 2, b = 1, ["end"] = 5, ["with space"] = 3 }"#,
            ]
        );
    }

    #[test]
    fn stops_at_cycles_and_deep_nesting() {
        let logs = logs(
            "{}",
            r#"
                local t = {}
                t.self = t
                console.log(t)
                console.log({ { { { { 1 } } } } })
                console.log({})
            "#,
        );
        assert_eq!(
            messages(&logs),
            ["{ self = <cycle> }", "{ { { { {...} } } } }", "{}"]
        );
    }

    #[test]
    fn maps_methods_to_levels() {
        let logs = logs(
            "{}",
            r#"
                console.log("l") console.info("i") console.warn("w")
                console.error("e") console.debug("d") print("p", { 1 })
            "#,
        );
        assert_eq!(
            logs,
            vec![
                StepLogEntry::new(Log, "l"),
                StepLogEntry::new(Info, "i"),
                StepLogEntry::new(Warn, "w"),
                StepLogEntry::new(Error, "e"),
                StepLogEntry::new(Log, "d"),
                StepLogEntry::new(Log, "p { 1 }"),
            ]
        );
    }

    #[test]
    fn asserts_log_only_failures() {
        let logs = logs(
            "{}",
            r#"
                console.assert(true, "hidden")
                console.assert(false)
                console.assert(1 == 2, "math is", "broken")
            "#,
        );
        assert_eq!(
            logs,
            vec![
                StepLogEntry::new(Error, "Assertion failed"),
                StepLogEntry::new(Error, "Assertion failed: math is broken"),
            ]
        );
    }

    #[test]
    fn counts_per_label() {
        let logs = logs(
            "{}",
            r#"
                console.count() console.count() console.count("x")
                console.countReset() console.count() console.countReset("missing")
            "#,
        );
        assert_eq!(
            logs,
            vec![
                StepLogEntry::new(Info, "count default: 1"),
                StepLogEntry::new(Info, "count default: 2"),
                StepLogEntry::new(Info, "count x: 1"),
                StepLogEntry::new(Info, "count default: 1"),
                StepLogEntry::new(Warn, "Count for 'missing' does not exist"),
            ]
        );
    }

    #[test]
    fn times_per_label() {
        let logs = logs(
            "{}",
            r#"
                console.time("t") console.time("t")
                console.timeLog("t", "halfway") console.timeEnd("t") console.timeEnd("t")
            "#,
        );
        assert_eq!(logs[0], StepLogEntry::new(Warn, "Timer 't' already exists"));
        assert_eq!(logs[1].level, Log);
        assert!(logs[1].message.starts_with("t: "), "{}", logs[1].message);
        assert!(
            logs[1].message.ends_with(" ms halfway"),
            "{}",
            logs[1].message
        );
        assert_eq!(logs[2].level, Info);
        assert!(
            logs[2].message.ends_with(" ms - timer removed"),
            "{}",
            logs[2].message
        );
        assert_eq!(logs[3], StepLogEntry::new(Warn, "Timer 't' does not exist"));
        assert_eq!(logs.len(), 4);
    }

    #[test]
    fn groups_indent_what_follows() {
        let logs = logs(
            "{}",
            r#"
                console.group("outer") console.log("a")
                console.groupCollapsed("inner") console.log("b\nc") console.groupEnd()
                console.log("d") console.groupEnd() console.groupEnd() console.log("e")
            "#,
        );
        assert_eq!(
            messages(&logs),
            [
                "group: outer",
                "  a",
                "  group: inner",
                "    b\n    c",
                "  d",
                "e",
            ]
        );
    }

    #[test]
    fn traces_the_calling_script_frames() {
        let logs = logs(
            "{}",
            "local function helper()\n  console.trace('here', 1)\nend\nhelper()\n",
        );
        assert_eq!(logs.len(), 1);
        let message = &logs[0].message;
        assert!(message.starts_with("here 1\nstack traceback:"), "{message}");
        assert!(message.contains("script:2: in"), "{message}");
        assert!(message.contains("script:4: in main chunk"), "{message}");
    }
}
