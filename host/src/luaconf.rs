//! Read the owner's real `init.lua`.
//!
//! `pin keymap_preservation` says the keymap baseline is the current keymap and
//! `pin keymap_no_redesign` forbids replacing it. The current keymap is a Lua
//! file, so the honest way to honour those pins is to *run* it, not to copy the
//! interesting lines into Rust — a copy is a second source that drifts, and the
//! drift is silent.
//!
//! `free neovim_glsl.lua_runtime_presence` leaves embedding Lua open, so this
//! takes it. What it does not take is the rest of Neovim: the `vim` table here
//! **records rather than executes**. `vim.opt.shiftwidth = 2` is an observation,
//! `nvim_set_keymap` is an observation, and anything else — plugin setup, LSP,
//! Treesitter — meets a permissive stub that answers every call with another
//! stub. So a config that configures forty plugins yields its options and its
//! keymaps, and none of the plugins run.
//!
//! Failure is partial, not total: the file is executed under `pcall`, so an
//! error part-way leaves everything recorded up to that point, and the error is
//! reported rather than swallowed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, Table, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    Bool(bool),
    Number(f64),
    Text(String),
    /// `listchars = { tab = '▸ ', trail = '·' }` and friends.
    Map(BTreeMap<String, String>),
    List(Vec<String>),
}

impl Setting {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Setting::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_usize(&self) -> Option<usize> {
        match self {
            Setting::Number(value) if *value >= 0.0 => Some(*value as usize),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        match self {
            Setting::Map(map) => map.get(key).map(String::as_str),
            _ => None,
        }
    }
}

/// One `nvim_set_keymap` / `vim.keymap.set` call.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    /// `n`, `i`, `v`, `x`, … as Neovim spells them.
    pub mode: String,
    pub lhs: String,
    pub rhs: String,
}

impl Mapping {
    /// `<cmd>Telescope find_files<cr>` and `:e ~/.config/nvim/init.lua<CR>` are
    /// commands; `y$` and `15j` are keys to replay.
    pub fn command(&self) -> Option<&str> {
        let rhs = self.rhs.trim();
        let lower = rhs.to_ascii_lowercase();
        let body = if let Some(rest) = lower.strip_prefix("<cmd>") {
            &rhs[5..5 + rest.trim_end_matches("<cr>").len()]
        } else if let Some(rest) = rhs.strip_prefix(':') {
            &rest[..rest.len()
                - rest
                    .to_ascii_lowercase()
                    .split("<cr>")
                    .last()
                    .map_or(0, str::len)]
        } else {
            return None;
        };
        Some(
            body.trim_end_matches("<CR>")
                .trim_end_matches("<cr>")
                .trim(),
        )
    }
}

#[derive(Debug, Default)]
pub struct NvimConfig {
    pub path: Option<PathBuf>,
    pub options: BTreeMap<String, Setting>,
    pub globals: BTreeMap<String, Setting>,
    pub mappings: Vec<Mapping>,
    /// What went wrong, if the file stopped early. Recorded, not swallowed.
    pub error: Option<String>,
}

impl NvimConfig {
    pub fn option(&self, name: &str) -> Option<&Setting> {
        self.options.get(name)
    }

    pub fn bool_option(&self, name: &str, fallback: bool) -> bool {
        self.option(name)
            .and_then(Setting::as_bool)
            .unwrap_or(fallback)
    }

    pub fn usize_option(&self, name: &str, fallback: usize) -> usize {
        self.option(name)
            .and_then(Setting::as_usize)
            .unwrap_or(fallback)
    }

    pub fn leader(&self) -> String {
        match self.globals.get("mapleader") {
            Some(Setting::Text(key)) if !key.is_empty() => key.clone(),
            _ => "\\".to_string(),
        }
    }

    /// Mappings for one mode, longest left-hand side first so that `ss` is
    /// matched before `s`.
    pub fn for_mode(&self, mode: char) -> Vec<&Mapping> {
        let mut found: Vec<&Mapping> = self
            .mappings
            .iter()
            .filter(|m| m.mode.contains(mode))
            .collect();
        found.sort_by_key(|m| std::cmp::Reverse(m.lhs.chars().count()));
        found
    }
}

/// Where Neovim itself would look.
pub fn default_config_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("NVIMGLSL_CONFIG") {
        let path = PathBuf::from(explicit);
        return path.is_file().then_some(path);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    let path = base.join("nvim/init.lua");
    path.is_file().then_some(path)
}

pub fn load_default() -> NvimConfig {
    match default_config_path() {
        Some(path) => load(&path),
        None => NvimConfig::default(),
    }
}

pub fn load(path: &Path) -> NvimConfig {
    let mut config = NvimConfig {
        path: Some(path.to_path_buf()),
        ..NvimConfig::default()
    };
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            config.error = Some(format!("{}: {error}", path.display()));
            return config;
        }
    };
    if let Err(error) = harvest(&mut config, path, &source) {
        config.error = Some(error.to_string());
    }
    config
}

fn harvest(config: &mut NvimConfig, path: &Path, source: &str) -> mlua::Result<()> {
    let lua = Lua::new();
    let globals = lua.globals();

    // Three tables the shim writes into and Rust reads back afterwards. Keeping
    // them in Lua rather than in Rust closures avoids threading a shared
    // RefCell through every callback.
    lua.load(RECORDER).set_name("nvimglsl:recorder").exec()?;

    // `require` resolves a sibling module out of the same config directory when
    // there is one, so a config split across files is read whole. Anything else
    // — every plugin — becomes a stub.
    let lua_dir = path.parent().map(|dir| dir.join("lua")).unwrap_or_default();
    globals.set("__nvimglsl_lua_dir", lua_dir.to_string_lossy().to_string())?;
    lua.load(REQUIRE).set_name("nvimglsl:require").exec()?;

    // Errors are contained: whatever ran before the failure stays recorded.
    let wrapped = lua.load(source).set_name("init.lua").into_function()?;
    let protected: mlua::Function = lua.load(PCALL).set_name("nvimglsl:pcall").eval()?;
    let error: Option<String> = protected.call(wrapped)?;

    let recorded: Table = globals.get("__nvimglsl")?;
    read_settings(&recorded.get::<Table>("options")?, &mut config.options)?;
    read_settings(&recorded.get::<Table>("globals")?, &mut config.globals)?;
    for entry in recorded.get::<Table>("maps")?.sequence_values::<Table>() {
        let entry = entry?;
        config.mappings.push(Mapping {
            mode: entry.get::<String>("mode")?,
            lhs: entry.get::<String>("lhs")?,
            rhs: entry.get::<String>("rhs")?,
        });
    }
    if let Some(error) = error {
        config.error = Some(error);
    }
    Ok(())
}

fn read_settings(table: &Table, into: &mut BTreeMap<String, Setting>) -> mlua::Result<()> {
    for pair in table.pairs::<String, Value>() {
        let (name, value) = pair?;
        if let Some(setting) = setting_from(&value)? {
            into.insert(name, setting);
        }
    }
    Ok(())
}

fn setting_from(value: &Value) -> mlua::Result<Option<Setting>> {
    Ok(match value {
        Value::Boolean(value) => Some(Setting::Bool(*value)),
        Value::Integer(value) => Some(Setting::Number(*value as f64)),
        Value::Number(value) => Some(Setting::Number(*value)),
        Value::String(value) => Some(Setting::Text(value.to_str()?.to_string())),
        Value::Table(table) => {
            let mut map = BTreeMap::new();
            let mut list = Vec::new();
            for pair in table.clone().pairs::<Value, Value>() {
                let (key, item) = pair?;
                let text = match &item {
                    Value::String(text) => text.to_str()?.to_string(),
                    Value::Integer(number) => number.to_string(),
                    Value::Number(number) => number.to_string(),
                    Value::Boolean(flag) => flag.to_string(),
                    _ => continue,
                };
                match key {
                    Value::String(key) => {
                        map.insert(key.to_str()?.to_string(), text);
                    }
                    Value::Integer(_) => list.push(text),
                    _ => {}
                }
            }
            if map.is_empty() {
                Some(Setting::List(list))
            } else {
                Some(Setting::Map(map))
            }
        }
        _ => None,
    })
}

/// The `vim` table, as a recorder.
///
/// Every assignment into `vim.opt` / `vim.o` / `vim.g` is stored; every keymap
/// call is stored; everything else answers with a stub that can be indexed,
/// called and assigned to forever without doing anything. That is what lets a
/// config which sets up LSP, Treesitter, completion and forty plugins run to
/// the end inside a program that has none of them.
const RECORDER: &str = r#"
__nvimglsl = { options = {}, globals = {}, maps = {}, commands = {} }

local function stub()
  local t = {}
  setmetatable(t, {
    __index = function() return stub() end,
    __call = function() return stub() end,
    __newindex = function() end,
    __concat = function() return "" end,
  })
  return t
end
__nvimglsl_stub = stub

-- `fallback_stub` decides what an *unset* key reads as, and the difference
-- matters more than it looks. `vim.opt.rtp:prepend(...)` needs a stub to call a
-- method on. `vim.g.vscode` needs nil: a stub is a table, a table is truthy in
-- Lua, and a truthy `vim.g.vscode` sends the whole config down its VSCode
-- branch — which reads as a successful load and yields the wrong keymap.
local function recorder(into, fallback_stub)
  local t = {}
  setmetatable(t, {
    __newindex = function(_, key, value) into[key] = value end,
    __index = function(_, key)
      local held = into[key]
      if held ~= nil then return held end
      if fallback_stub then return stub() end
      return nil
    end,
  })
  return t
end

local function modes(mode)
  if type(mode) == "table" then return table.concat(mode, "") end
  return tostring(mode)
end

local function record_map(mode, lhs, rhs)
  if type(rhs) ~= "string" then return end
  table.insert(__nvimglsl.maps, { mode = modes(mode), lhs = tostring(lhs), rhs = rhs })
end

vim = {}
vim.opt = recorder(__nvimglsl.options, true)
vim.o = vim.opt
vim.go = vim.opt
vim.wo = recorder({}, true)
vim.bo = recorder({}, true)
vim.g = recorder(__nvimglsl.globals, false)
vim.b = recorder({}, false)
vim.v = recorder({}, false)
vim.env = recorder({}, false)

vim.api = stub()
rawset(vim.api, "nvim_set_keymap", record_map)
rawset(vim.api, "nvim_buf_set_keymap", function(_, mode, lhs, rhs) record_map(mode, lhs, rhs) end)
vim.keymap = { set = record_map, del = function() end }

-- `vim.cmd` is called as a function *and* indexed as a table (`vim.cmd.split()`),
-- so it has to be a table with __call rather than a function.
vim.cmd = setmetatable({}, {
  __call = function(_, text)
    if type(text) == "string" then table.insert(__nvimglsl.commands, text) end
  end,
  __index = function() return function() end end,
})

vim.fn = stub()
-- Two functions whose *return value* the config actually uses. `stdpath` feeds
-- a string concatenation, and `fs_stat` answering truthy is what stops the
-- config from trying to git-clone a plugin manager on startup.
rawset(vim.fn, "stdpath", function() return "" end)
rawset(vim.fn, "expand", function(s) return tostring(s) end)
rawset(vim.fn, "has", function() return 0 end)
rawset(vim.fn, "system", function() return "" end)
rawset(vim.fn, "empty", function(v) return (v == nil or v == "") and 1 or 0 end)
vim.loop = stub()
rawset(vim.loop, "fs_stat", function() return { type = "directory" } end)
vim.uv = vim.loop

vim.tbl_deep_extend = function(_, a, b) return b or a or {} end
vim.tbl_extend = vim.tbl_deep_extend
vim.split = function(s) return { tostring(s) } end
vim.schedule = function(f) return f end
vim.defer_fn = function() end
vim.notify = function() end
vim.inspect = function() return "" end
vim.diagnostic = stub()
vim.lsp = stub()
vim.ui = stub()
vim.log = { levels = { ERROR = 1, WARN = 2, INFO = 3, DEBUG = 4, TRACE = 5 } }
"#;

/// `require` reads a sibling Lua module when the config has one, and otherwise
/// hands back a stub. Plugin specs therefore load — their keymaps are worth
/// having — while `require('telescope')` is inert.
const REQUIRE: &str = r#"
local real_require = require
local loaded = {}
require = function(name)
  if loaded[name] ~= nil then return loaded[name] end
  local path = __nvimglsl_lua_dir .. "/" .. tostring(name):gsub("%.", "/") .. ".lua"
  local chunk = loadfile(path)
  local value
  if chunk then
    local ok, result = pcall(chunk)
    value = (ok and result ~= nil) and result or __nvimglsl_stub()
  else
    value = __nvimglsl_stub()
  end
  loaded[name] = value
  return value
end
"#;

/// Run the config, returning the error text instead of raising it.
const PCALL: &str = r#"
return function(chunk)
  local ok, err = pcall(chunk)
  if ok then return nil end
  return tostring(err)
end
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn config_from(source: &str) -> NvimConfig {
        let dir = std::env::temp_dir().join(format!("nvimglsl-lua-{}", source.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("init.lua");
        std::fs::write(&path, source).unwrap();
        let config = load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        config
    }

    #[test]
    fn options_come_back_typed() {
        let config = config_from(
            "vim.opt.shiftwidth = 2\nvim.opt.expandtab = true\nvim.opt.listchars = { tab = '> ', trail = '.' }\n",
        );
        assert_eq!(config.error, None);
        assert_eq!(config.usize_option("shiftwidth", 8), 2);
        assert!(config.bool_option("expandtab", false));
        assert_eq!(config.option("listchars").unwrap().get("tab"), Some("> "));
    }

    #[test]
    fn both_keymap_spellings_are_recorded() {
        let config = config_from(
            "vim.api.nvim_set_keymap('n', 'Y', 'y$', {})\nvim.keymap.set('n', 'H', '^', {})\n",
        );
        assert_eq!(config.mappings.len(), 2);
        assert_eq!(config.for_mode('n')[0].rhs, "y$");
    }

    #[test]
    fn a_table_of_modes_becomes_one_entry_covering_them() {
        let config = config_from("vim.keymap.set({'n','x'}, 'gs', 'g_', {})\n");
        assert_eq!(config.for_mode('n').len(), 1);
        assert_eq!(config.for_mode('x').len(), 1);
        assert!(config.for_mode('i').is_empty());
    }

    #[test]
    fn longer_left_hand_sides_are_offered_first() {
        let config =
            config_from("vim.keymap.set('n', 's', 'a', {})\nvim.keymap.set('n', 'ss', 'b', {})\n");
        assert_eq!(config.for_mode('n')[0].lhs, "ss");
    }

    #[test]
    fn plugin_setup_runs_into_a_stub_instead_of_stopping_the_file() {
        let config = config_from(
            r#"
require('telescope').setup{ defaults = { layout_strategy = 'flex' } }
local lsp = require('lsp-zero').preset({ name = 'recommended' })
lsp.setup()
vim.opt.number = true
vim.api.nvim_set_keymap('n', 'Y', 'y$', {})
"#,
        );
        assert_eq!(
            config.error, None,
            "a stubbed plugin must not stop the config"
        );
        assert!(config.bool_option("number", false));
        assert_eq!(config.mappings.len(), 1);
    }

    #[test]
    fn a_failure_keeps_everything_recorded_before_it_and_says_what_broke() {
        let config = config_from(
            "vim.opt.number = true\nvim.api.nvim_set_keymap('n','Y','y$',{})\nerror('boom')\nvim.opt.wrap = true\n",
        );
        assert!(config.error.as_ref().unwrap().contains("boom"));
        assert!(config.bool_option("number", false));
        assert_eq!(config.mappings.len(), 1);
        assert!(config.option("wrap").is_none());
    }

    /// A config that branches on `vim.g.vscode` must take the branch Neovim
    /// would take. An unset global reading as a stub is truthy, and the wrong
    /// branch loads without error — the worst kind of wrong.
    #[test]
    fn an_unset_global_is_nil_rather_than_a_truthy_stub() {
        let config = config_from(
            "if vim.g.vscode then vim.api.nvim_set_keymap('n','a','WRONG',{}) else vim.api.nvim_set_keymap('n','a','RIGHT',{}) end\n",
        );
        assert_eq!(config.mappings[0].rhs, "RIGHT");
    }

    #[test]
    fn the_leader_is_read_rather_than_assumed() {
        let config = config_from("vim.g.mapleader = ' '\n");
        assert_eq!(config.leader(), " ");
        assert_eq!(config_from("vim.opt.number = true").leader(), "\\");
    }

    #[test]
    fn a_command_right_hand_side_is_told_apart_from_keys() {
        let cmd = Mapping {
            mode: "n".into(),
            lhs: "<space>o".into(),
            rhs: "<cmd>Telescope find_files<cr>".into(),
        };
        assert_eq!(cmd.command(), Some("Telescope find_files"));
        let keys = Mapping {
            mode: "n".into(),
            lhs: "Y".into(),
            rhs: "y$".into(),
        };
        assert_eq!(keys.command(), None);
    }

    /// The owner's own file, when it is there. Not a fixture: the point of this
    /// module is that it reads the real thing.
    #[test]
    fn the_real_config_yields_options_and_keymaps() {
        let Some(path) = default_config_path() else {
            return;
        };
        let config = load(&path);
        assert!(
            config.mappings.len() > 10,
            "only {} mappings from {}: {:?}",
            config.mappings.len(),
            path.display(),
            config.error
        );
        assert!(!config.options.is_empty());
    }
}
