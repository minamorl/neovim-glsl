//! Plugins, as spec v0.10 chose them: written in Lua, contributing surfaces.
//!
//! `pin plugin_mechanism` says there is an extension point, `pin
//! plugin_language_lua` that Lua is one of the languages it takes, `pin
//! plugin_surface_contribution` that a plugin can contribute a surface, and
//! `pin plugin_surface_form` / `pin plugin_surface_renderer` that the surface
//! is a **root-ui scene** which the **host** draws. So a plugin never issues a
//! draw call; it says what it wants drawn, in the same vocabulary the
//! navigation surface uses, and the host renders it.
//!
//! That split is not ceremony. A plugin that could draw would need the GL
//! context, the atlas and the frame loop, and every plugin would then be able
//! to break the frame for every other. Declaring a scene means the worst a
//! plugin can do to the screen is describe a bad one.
//!
//! ## What is not decided here
//!
//! `open_question plugin_effect_boundary` is open: nobody has said how much a
//! plugin may do. Running arbitrary Lua with the full standard library would
//! answer that question by default and in the least reversible direction, so
//! the environment below is cut down to what a surface plugin needs — no `io`,
//! no `os.execute`, no `package.loadlib`, no `require` of arbitrary paths.
//! That is a **provisional restriction, not a decision**: opening it later
//! needs no migration, and a plugin that quietly wrote files before anyone
//! chose the boundary could not be taken back.
//!
//! `open_question plugin_discovery` and `plugin_api_scope` are open too; the
//! directory and the function names below sit in `free plugin_layout` /
//! `free plugin_api_shape` and are this implementation's choices, not the
//! ledger's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, Table, Value};

use crate::root_ui::language::{
    rgba, Bounds, BoxKind, ColorIntent, ColorRuntime, ColorScheme, CornerRadius, Decoration,
    Sample, Semantic, Shadow,
};
use crate::root_ui::navigation::TextRun;
use crate::root_ui::FlatScene;

/// One loaded plugin.
pub struct Plugin {
    pub name: String,
    pub path: PathBuf,
    /// Ex commands it registered, in registration order.
    pub commands: Vec<String>,
    /// Surfaces it registered.
    pub surfaces: Vec<String>,
}

/// What a plugin's surface function returned, already turned into the shapes
/// the adapter draws.
pub struct PluginScene {
    pub scene: FlatScene,
    pub texts: Vec<TextRun>,
    /// Roles the plugin asked for that the scheme does not define. Reported
    /// rather than substituted: a silently recoloured surface is a plugin that
    /// looks like it works.
    pub unknown_roles: Vec<String>,
}

pub struct Host {
    lua: Lua,
    pub plugins: Vec<Plugin>,
    /// Command name to the plugin that owns it.
    commands: BTreeMap<String, usize>,
    surfaces: BTreeMap<String, usize>,
    /// Messages plugins asked to show, drained by the editor.
    pub messages: Vec<String>,
    pub errors: Vec<String>,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    pub fn new() -> Self {
        Self {
            lua: Lua::new(),
            plugins: Vec::new(),
            commands: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            messages: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Where plugins live. `free plugin_layout` — this is a choice, and
    /// `NVIMGLSL_PLUGINS` overrides it so a test never reads the owner's.
    pub fn default_directory() -> Option<PathBuf> {
        if let Some(explicit) = std::env::var_os("NVIMGLSL_PLUGINS") {
            return Some(PathBuf::from(explicit));
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("nvimglsl/plugins"))
    }

    pub fn load_default() -> Self {
        let mut host = Self::new();
        if let Some(directory) = Self::default_directory() {
            host.load_directory(&directory);
        }
        host
    }

    /// Load every `.lua` file in `directory`, in name order so that a load
    /// order exists at all and is the same on every run.
    pub fn load_directory(&mut self, directory: &Path) {
        let Ok(entries) = std::fs::read_dir(directory) else { return };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "lua"))
            .collect();
        paths.sort();
        for path in paths {
            if let Err(error) = self.load(&path) {
                self.errors.push(format!("{}: {error}", path.display()));
            }
        }
    }

    pub fn load(&mut self, path: &Path) -> mlua::Result<()> {
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let source = std::fs::read_to_string(path)
            .map_err(|error| mlua::Error::external(format!("{error}")))?;

        let index = self.plugins.len();
        self.plugins.push(Plugin {
            name: name.clone(),
            path: path.to_path_buf(),
            commands: Vec::new(),
            surfaces: Vec::new(),
        });
        self.install_api(index)?;
        // A plugin that fails to load leaves its registrations behind rather
        // than vanishing: half of it may already have registered, and pretending
        // otherwise would hide a command that exists.
        let result = self.lua.load(&source).set_name(name.as_str()).exec();

        let registry: Table = self.lua.globals().get("__nvimglsl_registry")?;
        for entry in registry.get::<Table>("commands")?.sequence_values::<String>() {
            let command = entry?;
            self.commands.insert(command.clone(), index);
            self.plugins[index].commands.push(command);
        }
        for entry in registry.get::<Table>("surfaces")?.sequence_values::<String>() {
            let surface = entry?;
            self.surfaces.insert(surface.clone(), index);
            self.plugins[index].surfaces.push(surface);
        }
        result
    }

    /// The table a plugin is handed, and the sandbox it runs in.
    fn install_api(&mut self, index: usize) -> mlua::Result<()> {
        let globals = self.lua.globals();
        globals.set("__nvimglsl_plugin_index", index)?;
        self.lua.load(API).set_name("nvimglsl:plugin-api").exec()
    }

    pub fn has_command(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    pub fn command_names(&self) -> Vec<&str> {
        self.commands.keys().map(String::as_str).collect()
    }

    pub fn surface_names(&self) -> Vec<&str> {
        self.surfaces.keys().map(String::as_str).collect()
    }

    /// Run a registered command. `argument` is whatever followed the name.
    pub fn run_command(&mut self, name: &str, argument: &str) -> Result<(), String> {
        if !self.commands.contains_key(name) {
            return Err(format!("no plugin command named {name}"));
        }
        let call: mlua::Function = self
            .lua
            .load("return function(name, arg) return __nvimglsl_call_command(name, arg) end")
            .eval()
            .map_err(|error| error.to_string())?;
        call.call::<()>((name, argument)).map_err(|error| error.to_string())?;
        self.drain_messages();
        Ok(())
    }

    /// Ask a registered surface for its scene.
    pub fn surface(&mut self, name: &str, window: (f32, f32), scale: f32) -> Result<PluginScene, String> {
        if !self.surfaces.contains_key(name) {
            return Err(format!("no plugin surface named {name}"));
        }
        let call: mlua::Function = self
            .lua
            .load("return function(name, w, h) return __nvimglsl_call_surface(name, w, h) end")
            .eval()
            .map_err(|error| error.to_string())?;
        let table: Table =
            call.call((name, window.0, window.1)).map_err(|error| error.to_string())?;
        let scene = read_scene(&table, name, window, scale).map_err(|error| error.to_string())?;
        self.drain_messages();
        Ok(scene)
    }

    fn drain_messages(&mut self) {
        let Ok(globals) = self.lua.globals().get::<Table>("__nvimglsl_registry") else { return };
        let Ok(messages) = globals.get::<Table>("messages") else { return };
        for entry in messages.clone().sequence_values::<String>().flatten() {
            self.messages.push(entry);
        }
        let _ = globals.set("messages", self.lua.create_table().unwrap_or_else(|_| messages));
    }
}

/// Turn a plugin's table into root-ui samples.
///
/// Positions are fractions of the window, which is root-ui's own normalized
/// layout, so a plugin never needs to know the pixel size of anything. Radii
/// and shadows are density-independent pixels, which is the unit root-ui gained
/// for exactly this reason: a corner should be the same size on every surface.
fn read_scene(
    table: &Table,
    plugin: &str,
    window: (f32, f32),
    scale: f32,
) -> mlua::Result<PluginScene> {
    let mut surfaces = Vec::new();
    let mut texts = Vec::new();
    let mut unknown_roles = Vec::new();
    let known: Vec<&str> = crate::root_ui::navigation::ROLES.to_vec();
    let mut check = |role: &str, unknown: &mut Vec<String>| {
        if !known.contains(&role) && !unknown.iter().any(|seen| seen == role) {
            unknown.push(role.to_string());
        }
    };

    if let Ok(list) = table.get::<Table>("surfaces") {
        for (index, entry) in list.sequence_values::<Table>().enumerate() {
            let entry = entry?;
            let number = |key: &str, fallback: f32| -> f32 {
                entry.get::<f32>(key).unwrap_or(fallback)
            };
            let text = |key: &str, fallback: &str| -> String {
                entry.get::<String>(key).unwrap_or_else(|_| fallback.to_string())
            };
            let fill = text("fill", "surface");
            let stroke = text("stroke", &fill);
            check(&fill, &mut unknown_roles);
            check(&stroke, &mut unknown_roles);
            let radius = number("radius", 0.0);
            let shadow = entry.get::<Table>("shadow").ok().map(|shadow| Shadow {
                offset_x: CornerRadius::Pixels(shadow.get::<f32>("dx").unwrap_or(0.0)),
                offset_y: CornerRadius::Pixels(shadow.get::<f32>("dy").unwrap_or(0.0)),
                blur: CornerRadius::Pixels(shadow.get::<f32>("blur").unwrap_or(0.0).max(0.0)),
                spread: CornerRadius::Pixels(shadow.get::<f32>("spread").unwrap_or(0.0).max(0.0)),
            });
            if shadow.is_some() {
                check("shadow", &mut unknown_roles);
            }
            let x = number("x", 0.0).clamp(0.0, 1.0);
            let y = number("y", 0.0).clamp(0.0, 1.0);
            surfaces.push((
                format!("plugin.{plugin}.{}", entry.get::<String>("id").unwrap_or(index.to_string())),
                Sample {
                    semantic: Semantic::new(
                        &text("name", "Surface"),
                        "plugin",
                        &text("state", "rest"),
                    ),
                    kind: if radius > 0.0 { BoxKind::RoundBox } else { BoxKind::Box },
                    bounds: Bounds {
                        x,
                        y,
                        width: number("w", 0.0).clamp(0.0, 1.0 - x),
                        height: number("h", 0.0).clamp(0.0, 1.0 - y),
                    },
                    decoration: Decoration {
                        stroke_width: number("stroke_width", 0.0).clamp(0.0, 0.5),
                        shadow,
                    },
                    color: match shadow {
                        Some(_) => ColorIntent::new(&fill, &stroke).with_shadow("shadow"),
                        None => ColorIntent::new(&fill, &stroke),
                    },
                    corner_radius: CornerRadius::Pixels(radius),
                },
            ));
        }
    }

    if let Ok(list) = table.get::<Table>("texts") {
        for entry in list.sequence_values::<Table>() {
            let entry = entry?;
            let role = entry.get::<String>("role").unwrap_or_else(|_| "on_surface".to_string());
            check(&role, &mut unknown_roles);
            // The role has to outlive the frame, and the set is closed, so a
            // known role becomes its 'static name and an unknown one falls back
            // to a readable colour rather than to nothing.
            let role = crate::root_ui::navigation::ROLES
                .iter()
                .find(|known| **known == role)
                .copied()
                .unwrap_or("on_surface");
            texts.push(TextRun {
                x: entry.get::<f32>("x").unwrap_or(0.0).clamp(0.0, 1.0) * window.0,
                baseline: entry.get::<f32>("y").unwrap_or(0.0).clamp(0.0, 1.0) * window.1,
                text: entry.get::<String>("text").unwrap_or_default(),
                role,
                max_x: entry.get::<f32>("max_x").unwrap_or(1.0).clamp(0.0, 1.0) * window.0,
            });
        }
    }

    let _ = scale;
    Ok(PluginScene { scene: FlatScene { surfaces }, texts, unknown_roles })
}

/// A scheme with the plugin roles in it, which is the navigation scheme: one
/// visual language, not one per extension.
pub fn color_runtime(scheme: &str) -> ColorRuntime {
    crate::root_ui::navigation::color_runtime(scheme)
}

pub fn scheme_named(id: &str) -> ColorScheme {
    if id == "light" {
        crate::root_ui::navigation::light_scheme()
    } else {
        crate::root_ui::navigation::dark_scheme()
    }
}

/// Kept so a caller can build a colour without reaching into root-ui.
pub fn color(hex: &str, alpha: f32) -> [f32; 4] {
    rgba(hex, alpha)
}

/// The `nvimglsl` table a plugin sees, plus the sandbox.
///
/// Removing `io`, `os.execute`, `os.remove`, `os.rename`, `dofile`,
/// `loadfile`, `package.loadlib` and `require` is the provisional stance
/// described at the top of this file, not an answer to
/// `open_question plugin_effect_boundary`.
const API: &str = r#"
__nvimglsl_registry = __nvimglsl_registry
  or { commands = {}, surfaces = {}, messages = {}, handlers = {}, providers = {} }
local registry = __nvimglsl_registry
registry.commands = {}
registry.surfaces = {}

nvimglsl = {
  command = function(name, fn)
    if type(name) ~= "string" or type(fn) ~= "function" then
      error("nvimglsl.command(name, fn) takes a name and a function")
    end
    registry.handlers[name] = fn
    table.insert(registry.commands, name)
  end,
  surface = function(name, fn)
    if type(name) ~= "string" or type(fn) ~= "function" then
      error("nvimglsl.surface(name, fn) takes a name and a function")
    end
    registry.providers[name] = fn
    table.insert(registry.surfaces, name)
  end,
  notify = function(text)
    table.insert(registry.messages, tostring(text))
  end,
}

function __nvimglsl_call_command(name, argument)
  local handler = registry.handlers[name]
  if handler == nil then error("no command " .. tostring(name)) end
  return handler(argument)
end

function __nvimglsl_call_surface(name, width, height)
  local provider = registry.providers[name]
  if provider == nil then error("no surface " .. tostring(name)) end
  local scene = provider({ width = width, height = height })
  if type(scene) ~= "table" then
    error("surface " .. tostring(name) .. " returned " .. type(scene) .. ", not a scene table")
  end
  return scene
end

-- The provisional boundary. See the module comment: a restriction now is
-- reversible, and a plugin that wrote files before anyone chose is not.
io = nil
dofile = nil
loadfile = nil
require = nil
if package then package.loadlib = nil end
if os then
  os.execute = nil
  os.remove = nil
  os.rename = nil
  os.tmpname = nil
  os.exit = nil
  os.getenv = nil
end
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_dir(name: &str, sources: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nvimglsl-plugins-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, source) in sources {
            std::fs::write(dir.join(file), source).unwrap();
        }
        dir
    }

    fn host_with(name: &str, sources: &[(&str, &str)]) -> Host {
        let dir = plugin_dir(name, sources);
        let mut host = Host::new();
        host.load_directory(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        host
    }

    #[test]
    fn a_plugin_registers_a_command_and_it_runs() {
        let mut host = host_with(
            "command",
            &[("hello.lua", "nvimglsl.command('Hello', function(arg) nvimglsl.notify('hi ' .. tostring(arg)) end)")],
        );
        assert!(host.errors.is_empty(), "{:?}", host.errors);
        assert!(host.has_command("Hello"));
        host.run_command("Hello", "there").unwrap();
        assert_eq!(host.messages, vec!["hi there"]);
    }

    #[test]
    fn a_plugin_surface_becomes_root_ui_samples_the_host_draws() {
        let mut host = host_with(
            "surface",
            &[(
                "panel.lua",
                r#"
nvimglsl.surface('Panel', function(window)
  return {
    surfaces = {
      { id = 'bg', x = 0.1, y = 0.2, w = 0.5, h = 0.25, radius = 10,
        fill = 'surface', stroke = 'outline', shadow = { dy = 8, blur = 20 } },
    },
    texts = { { x = 0.12, y = 0.3, text = 'from a plugin', role = 'accent' } },
  }
end)
"#,
            )],
        );
        assert!(host.errors.is_empty(), "{:?}", host.errors);
        let scene = host.surface("Panel", (1000.0, 800.0), 1.0).unwrap();
        assert_eq!(scene.scene.surfaces.len(), 1);
        assert_eq!(scene.scene.surfaces[0].0, "plugin.Panel.bg");
        assert!(scene.unknown_roles.is_empty());
        assert_eq!(scene.texts[0].text, "from a plugin");
        assert_eq!(scene.texts[0].role, "accent");
        // The declared scene has to survive the same phases every other surface
        // goes through, or the host cannot draw it.
        let prepared = crate::root_ui::prepare_flat_scene(&scene.scene).expect("prepared");
        crate::root_ui::bind_flat_scene_user_color_scheme(&prepared, &color_runtime("dark"))
            .expect("bound");
    }

    #[test]
    fn a_role_the_scheme_does_not_define_is_reported_rather_than_substituted() {
        let mut host = host_with(
            "roles",
            &[(
                "bad.lua",
                "nvimglsl.surface('S', function() return { surfaces = { { x=0,y=0,w=1,h=1, fill='chartreuse' } } } end)",
            )],
        );
        let scene = host.surface("S", (100.0, 100.0), 1.0).unwrap();
        assert_eq!(scene.unknown_roles, vec!["chartreuse"]);
    }

    #[test]
    fn a_plugin_cannot_reach_the_filesystem_or_spawn_anything() {
        // Provisional, and stated as such in the module comment — but it has to
        // actually hold, or the note is decoration.
        let host = host_with(
            "sandbox",
            &[(
                "probe.lua",
                r#"
local blocked = {}
for _, name in ipairs({ 'io', 'dofile', 'loadfile', 'require' }) do
  if _G[name] ~= nil then table.insert(blocked, name) end
end
for _, name in ipairs({ 'execute', 'remove', 'rename', 'exit', 'getenv' }) do
  if os and os[name] ~= nil then table.insert(blocked, 'os.' .. name) end
end
if package and package.loadlib ~= nil then table.insert(blocked, 'package.loadlib') end
if #blocked > 0 then error('reachable: ' .. table.concat(blocked, ', ')) end
"#,
            )],
        );
        assert!(host.errors.is_empty(), "{:?}", host.errors);
    }

    #[test]
    fn a_broken_plugin_reports_itself_and_does_not_stop_the_others() {
        let host = host_with(
            "broken",
            &[
                ("a-good.lua", "nvimglsl.command('Good', function() end)"),
                ("b-bad.lua", "error('this plugin is broken')"),
                ("c-also-good.lua", "nvimglsl.command('AlsoGood', function() end)"),
            ],
        );
        assert_eq!(host.errors.len(), 1);
        assert!(host.errors[0].contains("this plugin is broken"));
        assert!(host.has_command("Good") && host.has_command("AlsoGood"));
    }

    #[test]
    fn registrations_a_failing_plugin_made_before_it_broke_are_kept() {
        let host = host_with(
            "partial",
            &[("half.lua", "nvimglsl.command('Half', function() end)\nerror('later')")],
        );
        assert_eq!(host.errors.len(), 1);
        assert!(host.has_command("Half"), "a registered command must not vanish");
    }

    #[test]
    fn plugins_load_in_name_order_so_the_order_exists_and_repeats() {
        let host = host_with(
            "order",
            &[
                ("2-second.lua", "nvimglsl.command('Second', function() end)"),
                ("1-first.lua", "nvimglsl.command('First', function() end)"),
            ],
        );
        let names: Vec<&str> = host.plugins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["1-first", "2-second"]);
    }

    #[test]
    fn registering_with_the_wrong_shape_is_an_error_not_a_silent_no_op() {
        let host = host_with("shape", &[("bad.lua", "nvimglsl.command('X', 'not a function')")]);
        assert_eq!(host.errors.len(), 1);
        assert!(host.errors[0].contains("takes a name and a function"));
    }

    #[test]
    fn a_surface_that_returns_the_wrong_thing_says_so() {
        let mut host =
            host_with("wrong", &[("w.lua", "nvimglsl.surface('W', function() return 42 end)")]);
        let error = match host.surface("W", (100.0, 100.0), 1.0) {
            Err(error) => error,
            Ok(_) => panic!("a non-table return must not be accepted"),
        };
        assert!(error.contains("not a scene table"), "{error}");
    }

    #[test]
    fn an_absent_plugin_directory_is_no_plugins_rather_than_a_failure() {
        let mut host = Host::new();
        host.load_directory(Path::new("/nonexistent/nvimglsl/plugins"));
        assert!(host.plugins.is_empty() && host.errors.is_empty());
    }

    #[test]
    fn bounds_outside_the_window_are_clamped_rather_than_refused_by_layout() {
        let mut host = host_with(
            "clamp",
            &[("c.lua", "nvimglsl.surface('C', function() return { surfaces = { { x=0.9, y=0.9, w=5, h=5 } } } end)")],
        );
        let scene = host.surface("C", (100.0, 100.0), 1.0).unwrap();
        // root-ui refuses bounds that leave the target, so a plugin asking for
        // them must be brought inside rather than dropped.
        crate::root_ui::prepare_flat_scene(&scene.scene).expect("clamped bounds must resolve");
    }
}
