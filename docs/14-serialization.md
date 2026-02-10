# 14. Settings Persistence with Serde

This milestone added automatic save/load of user preferences using serde serialization to a JSON config file.

## The Problem

Every time you close scope-rs, all your settings are lost -- display mode, color theme, gain, zoom, channel controls. You have to reconfigure everything on each launch.

## The Solution

Serialize settings to `~/.config/scope-rs/settings.json` on exit, deserialize on startup.

```
Startup                                    Shutdown
   |                                          |
   v                                          v
settings.json exists?                    Extract settings
   |           |                         from ScopeApp
  yes         no                              |
   |           |                              v
   v           v                      Serialize to JSON
 Parse      Use defaults                      |
  JSON         |                              v
   |           |                     Write settings.json
   v           v
 Apply to ScopeApp
```

## Serde: Rust's Serialization Framework

Serde is Rust's standard approach to serialization. The name comes from **ser**ialize / **de**serialize.

### Derive Macros

The simplest way to use serde is with derive macros:

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub enum DisplayMode {
    Dots,
    Lines,
    Gradient,
    Points,
}

#[derive(Serialize, Deserialize)]
pub struct AppSettings {
    pub display_mode: DisplayMode,
    pub line_width: f32,
    pub zoom: f32,
    pub swap_xy: bool,
    // ...
}
```

The derive macros automatically generate serialization code at compile time. No runtime reflection needed -- it's all resolved statically.

### How It Works

Serde uses a **data model** as an intermediate representation:

```
Rust struct  -->  Serde Data Model  -->  JSON/TOML/YAML/etc.
AppSettings       (generic format)       {"display_mode": "Lines", ...}
```

This separation means:
- `Serialize`/`Deserialize` derives are format-agnostic
- Switching from JSON to TOML only changes one line of code
- Custom types only need to implement serde traits once

### serde_json

We use `serde_json` as the concrete format:

```rust
// Serialize to pretty JSON string
let json = serde_json::to_string_pretty(&settings)?;

// Deserialize from JSON string
let settings: AppSettings = serde_json::from_str(&json)?;
```

The produced file is human-readable and hand-editable:

```json
{
  "display_mode": "Lines",
  "color_theme": "Green",
  "line_width": 1.5,
  "intensity": 1.0,
  "persistence": 0.85,
  "zoom": 1.0,
  "swap_xy": false,
  "invert_x": false,
  "invert_y": false,
  "dc_offset_x": 0.0,
  "dc_offset_y": 0.0,
  "gain": 1.0,
  "volume": 1.0,
  "speed": 1.0,
  "loop_enabled": false,
  "show_settings": false
}
```

## Forward Compatibility with `#[serde(default)]`

A key design decision: what happens when you add a new setting in a future version, but the user has an old config file?

```rust
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub display_mode: DisplayMode,
    pub line_width: f32,
    pub new_future_setting: bool,  // Not in old config files!
}
```

Without `#[serde(default)]`, deserializing an old file would **fail** because the new field is missing. With it, missing fields silently use their `Default` value.

This is critical for user-facing config files -- you never want an app upgrade to break someone's settings.

### How `#[serde(default)]` Works

| Attribute | Scope | Behavior |
|-----------|-------|----------|
| `#[serde(default)]` on struct | All fields | Missing fields use `Default::default()` for each type |
| `#[serde(default)]` on field | Single field | That field uses its type's `Default` |
| `#[serde(default = "path")]` | Single field | Uses a custom function for the default |

We put it on the struct level so every field is covered automatically.

## Architecture: Why a Separate `AppSettings` Struct?

We don't serialize `OscilloscopeSettings` directly because it contains `Color32` (an egui type that isn't serde-compatible). Instead:

```
OscilloscopeSettings              AppSettings
├── color: Color32       <---->   (not stored)
├── background: Color32  <---->   (not stored)
├── theme: ColorTheme    <---->   color_theme: ColorTheme
├── line_width: f32      <---->   line_width: f32
├── display_mode         <---->   display_mode: DisplayMode
└── ...                           ...
```

On save, `from_app()` extracts the serializable fields. On load, `apply()` restores them -- using `apply_theme()` to regenerate the `Color32` values from the theme enum.

This pattern is common: runtime state often contains types that don't serialize cleanly (handles, references, computed values). A dedicated settings struct acts as a clean serialization boundary.

## The `Drop` Trait for Auto-Save

We use Rust's `Drop` trait to save settings when the app exits:

```rust
impl Drop for ScopeApp {
    fn drop(&mut self) {
        AppSettings::from_app(self).save();
    }
}
```

### How Drop Works

`Drop` is called automatically when a value goes out of scope:

```rust
{
    let app = ScopeApp::new();
    // ... app runs ...
}  // <-- app.drop() called here automatically
```

Properties:
- **Deterministic** -- called at a known point (end of scope), not by a garbage collector
- **Guaranteed** -- runs even during panic unwinding (by default)
- **One chance** -- you can't call `drop()` manually (use `std::mem::drop()` to drop early)

This makes it perfect for cleanup operations like saving state.

## The `dirs` Crate

Finding the right config directory across platforms is surprisingly complex:

| Platform | Config Directory |
|----------|-----------------|
| Linux | `$XDG_CONFIG_HOME` or `~/.config` |
| macOS | `~/Library/Application Support` |
| Windows | `{FOLDERID_RoamingAppData}` (e.g., `C:\Users\Name\AppData\Roaming`) |

The `dirs` crate handles all of this:

```rust
use std::path::PathBuf;

fn settings_path() -> PathBuf {
    let mut path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    path.push("scope-rs");
    path.push("settings.json");
    path
}
```

## Graceful Error Handling

Settings are non-critical -- the app should always start, even with a corrupt or missing config:

```rust
pub fn load() -> Self {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(settings) => settings,
            Err(e) => {
                log::warn!("Failed to parse settings: {}", e);
                Self::default()
            }
        },
        Err(e) => {
            log::info!("No settings file: {}", e);
            Self::default()
        }
    }
}
```

Pattern: **load errors fall back to defaults, save errors are logged but never panic.** The user's experience is never blocked by a settings problem.

## Enum Serialization

Serde serializes Rust enums as strings by default:

```rust
#[derive(Serialize, Deserialize)]
pub enum ColorTheme {
    Green,
    Amber,
    Blue,
}
```

```json
{"color_theme": "Green"}
```

This is human-readable and stable. If you rename a variant, old config files will fail to deserialize that field (but `#[serde(default)]` catches it). You can use `#[serde(alias = "OldName")]` to handle renames gracefully.

## Key Takeaways

1. **Derive macros are powerful** -- `#[derive(Serialize, Deserialize)]` generates all the code you need, with zero boilerplate

2. **`#[serde(default)]` is essential for config files** -- ensures forward/backward compatibility as your settings evolve

3. **Separate serialization from runtime types** -- not everything in your app state belongs in a config file (handles, computed values, non-serializable types)

4. **`Drop` gives deterministic cleanup** -- perfect for auto-save, resource release, and other shutdown tasks

5. **Fail gracefully on settings errors** -- never let a corrupt config file prevent the app from starting

6. **Use `dirs` for cross-platform paths** -- don't hardcode `~/.config`

## Links

- [Serde documentation](https://serde.rs/)
- [serde_json](https://docs.rs/serde_json/)
- [dirs crate](https://docs.rs/dirs/)
- [Drop trait](https://doc.rust-lang.org/std/ops/trait.Drop.html)
- [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/)
