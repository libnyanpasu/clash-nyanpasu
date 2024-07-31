use mlua::prelude::*;
mod module;

pub fn create_lua_context() -> Lua {
    let lua = Lua::new();

    lua
}
