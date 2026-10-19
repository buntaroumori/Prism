//! Plugin manifests: `plugin.toml` parsing and validation.
//!
//! A manifest is the entire trust boundary: it declares the runtime (inferred
//! from the entry extension), the capabilities the plugin may use, and the
//! settings schema the configuration UI renders. Nothing declared here is
//! trusted until [`Manifest::load`] has validated it, and nothing outside the
//! plugin's own directory can be named as an entry point.
//!
//! ```toml
//! [plugin]
//! id = "io.prism.night-mode"
//! name = "Night Mode"
//! version = "1.0.0"
//! api = 1
//! entry = "main.lua"
//! description = "Lowers the volume after a given hour."
//! author = "Prism Contributors"
//! capabilities = ["log", "notify", "player:read", "player:control"]
//!
//! [[settings]]
//! key = "after_hour"
//! label = "After hour"
//! type = "integer"
//! default = 22
//! min = 0
//! max = 23
//! ```

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::PluginError;

/// Plugin API revision understood by this build.
pub const SUPPORTED_API: u32 = 1;
/// Manifest filename inside a plugin directory.
pub const MANIFEST_FILENAME: &str = "plugin.toml";
/// Upper bound on declared settings: a plugin asking for more than this is a
/// configuration panel nobody will read.
pub const MAX_SETTINGS: usize = 32;
/// Longest accepted free-text field (description, author, label, help).
const MAX_TEXT: usize = 280;
/// Longest accepted setting string value.
const MAX_VALUE_TEXT: usize = 512;

/// Which sandbox executes the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Lua,
    Wasm,
}

impl RuntimeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lua => "lua",
            Self::Wasm => "wasm",
        }
    }
}

/// A host capability. Everything a plugin can do beyond computing numbers is
/// gated behind one of these, and the manifest must spell them out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Capability {
    #[serde(rename = "log")]
    Log,
    #[serde(rename = "notify")]
    Notify,
    #[serde(rename = "player:read")]
    PlayerRead,
    #[serde(rename = "player:control")]
    PlayerControl,
}

impl Capability {
    /// Every capability the host understands, in display order.
    pub const ALL: [Self; 4] = [Self::Log, Self::Notify, Self::PlayerRead, Self::PlayerControl];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::Notify => "notify",
            Self::PlayerRead => "player:read",
            Self::PlayerControl => "player:control",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, PluginError> {
        Self::ALL
            .into_iter()
            .find(|capability| capability.as_str() == raw)
            .ok_or_else(|| {
                PluginError::manifest(format!(
                    "unknown capability '{raw}' (expected one of {})",
                    Self::ALL
                        .iter()
                        .map(|value| value.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }
}

/// The type a setting exposes to the configuration UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    String,
    Number,
    Integer,
    Boolean,
    Select,
}

impl SettingKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Select => "select",
        }
    }
}

/// One configurable value, rendered as a control by the settings panel.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingSpec {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: SettingKind,
    pub default: Value,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub help: Option<String>,
}

impl SettingSpec {
    /// Validate a candidate value and return the normalised form.
    ///
    /// Rejects rather than clamps: silently turning `120` into `100` behind the
    /// user's back is how a plugin ends up "working" while doing something else.
    pub fn coerce(&self, raw: &Value) -> Result<Value, PluginError> {
        let fail = |reason: &str| {
            Err(PluginError::manifest(format!(
                "setting '{}' {reason}",
                self.key
            )))
        };
        match self.kind {
            SettingKind::String => match raw {
                Value::String(text) if text.chars().count() <= MAX_VALUE_TEXT => Ok(raw.clone()),
                Value::String(_) => fail(&format!("must be at most {MAX_VALUE_TEXT} characters")),
                _ => fail("expects a string"),
            },
            SettingKind::Number => {
                let Value::Number(_) = raw else {
                    return fail("expects a number");
                };
                let Some(value) = raw.as_f64() else {
                    return fail("is not representable");
                };
                if !value.is_finite() {
                    return fail("must be finite");
                }
                self.check_range(value)?;
                Ok(raw.clone())
            }
            SettingKind::Integer => {
                let Value::Number(_) = raw else {
                    return fail("expects an integer");
                };
                let Some(value) = raw.as_i64() else {
                    return fail("expects an integer");
                };
                let as_float = value as f64;
                self.check_range(as_float)?;
                Ok(Value::from(value))
            }
            SettingKind::Boolean => match raw {
                Value::Bool(_) => Ok(raw.clone()),
                _ => fail("expects true or false"),
            },
            SettingKind::Select => match raw {
                Value::String(text) if self.options.iter().any(|option| option == text) => {
                    Ok(raw.clone())
                }
                Value::String(text) => fail(&format!("must be one of the declared options, got '{text}'")),
                _ => fail("expects a string option"),
            },
        }
    }

    fn check_range(&self, value: f64) -> Result<(), PluginError> {
        if let Some(min) = self.min
            && value < min
        {
            return Err(PluginError::manifest(format!(
                "setting '{}' must be >= {min}",
                self.key
            )));
        }
        if let Some(max) = self.max
            && value > max
        {
            return Err(PluginError::manifest(format!(
                "setting '{}' must be <= {max}",
                self.key
            )));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), PluginError> {
        if !is_valid_key(&self.key) {
            return Err(PluginError::manifest(format!(
                "invalid setting key '{}' (lowercase letters, digits and '_', starting with a letter)",
                self.key
            )));
        }
        if self.label.trim().is_empty() || self.label.chars().count() > MAX_TEXT {
            return Err(PluginError::manifest(format!(
                "setting '{}' needs a label of 1..={MAX_TEXT} characters",
                self.key
            )));
        }
        if let Some(help) = &self.help
            && help.chars().count() > MAX_TEXT
        {
            return Err(PluginError::manifest(format!(
                "setting '{}' help text exceeds {MAX_TEXT} characters",
                self.key
            )));
        }
        if let (Some(min), Some(max)) = (self.min, self.max)
            && min > max
        {
            return Err(PluginError::manifest(format!(
                "setting '{}' declares min > max",
                self.key
            )));
        }
        if self.kind == SettingKind::Select {
            if self.options.is_empty() {
                return Err(PluginError::manifest(format!(
                    "select setting '{}' declares no options",
                    self.key
                )));
            }
            if self.options.len() > MAX_SETTINGS {
                return Err(PluginError::manifest(format!(
                    "select setting '{}' declares more than {MAX_SETTINGS} options",
                    self.key
                )));
            }
            let unique: BTreeSet<&String> = self.options.iter().collect();
            if unique.len() != self.options.len()
                || self
                    .options
                    .iter()
                    .any(|option| option.trim().is_empty() || option.chars().count() > MAX_VALUE_TEXT)
            {
                return Err(PluginError::manifest(format!(
                    "select setting '{}' has duplicate or empty options",
                    self.key
                )));
            }
        } else if !self.options.is_empty() {
            return Err(PluginError::manifest(format!(
                "setting '{}' declares options but is not a select",
                self.key
            )));
        }
        if matches!(self.kind, SettingKind::String | SettingKind::Boolean)
            && (self.min.is_some() || self.max.is_some())
        {
            return Err(PluginError::manifest(format!(
                "setting '{}' declares a numeric range on a non-numeric type",
                self.key
            )));
        }
        // The default must itself satisfy the schema, otherwise the first launch
        // would start with an invalid configuration.
        self.coerce(&self.default)?;
        Ok(())
    }

    fn value_as_json(&self, value: &Value) -> Value {
        value.clone()
    }
}

/// Validated manifest.
#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u32,
    pub description: String,
    pub author: String,
    pub homepage: Option<String>,
    pub runtime: RuntimeKind,
    /// Entry file relative to the plugin directory.
    pub entry: PathBuf,
    pub capabilities: Vec<Capability>,
    pub settings: Vec<SettingSpec>,
}

impl Manifest {
    /// Read and validate `plugin.toml` inside `dir`.
    pub fn load(dir: &Path) -> Result<Self, PluginError> {
        let path = dir.join(MANIFEST_FILENAME);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| PluginError::io(path.clone(), source))?;
        if text.len() > 64 * 1024 {
            return Err(PluginError::manifest(format!(
                "{} exceeds 64 KiB",
                path.display()
            )));
        }
        let file: ManifestFile = toml::from_str(&text)
            .map_err(|error| PluginError::manifest(format!("{}: {error}", path.display())))?;
        Self::validate(dir, file)
    }

    fn validate(dir: &Path, file: ManifestFile) -> Result<Self, PluginError> {
        let meta = file.plugin;

        if !is_valid_id(&meta.id) {
            return Err(PluginError::manifest(format!(
                "invalid id '{}' (reverse-DNS, lowercase a-z0-9.-, 3..=64 characters, e.g. io.prism.night-mode)",
                meta.id
            )));
        }
        if meta.name.trim().is_empty() || meta.name.chars().count() > MAX_TEXT {
            return Err(PluginError::manifest(format!(
                "plugin '{}' needs a name of 1..={MAX_TEXT} characters",
                meta.id
            )));
        }
        if !is_valid_version(&meta.version) {
            return Err(PluginError::manifest(format!(
                "invalid version '{}' (expected MAJOR.MINOR.PATCH)",
                meta.version
            )));
        }
        if meta.api != SUPPORTED_API {
            return Err(PluginError::manifest(format!(
                "plugin targets API {} but this build implements API {SUPPORTED_API}",
                meta.api
            )));
        }
        if meta.description.chars().count() > MAX_TEXT || meta.author.chars().count() > MAX_TEXT {
            return Err(PluginError::manifest(format!(
                "plugin '{}' description/author exceed {MAX_TEXT} characters",
                meta.id
            )));
        }
        if let Some(homepage) = &meta.homepage
            && !(homepage.starts_with("https://") || homepage.starts_with("http://"))
        {
            return Err(PluginError::manifest(format!(
                "plugin '{}' homepage must be an http(s) URL",
                meta.id
            )));
        }

        let entry = PathBuf::from(meta.entry.trim());
        let runtime = runtime_for(&entry).ok_or_else(|| {
            PluginError::manifest(format!(
                "entry '{}' must end in .lua (Lua runtime) or .wasm/.wat (WebAssembly runtime)",
                entry.display()
            ))
        })?;
        validate_relative(&entry)?;
        let absolute = dir.join(&entry);
        if !absolute.is_file() {
            return Err(PluginError::manifest(format!(
                "entry '{}' does not exist",
                absolute.display()
            )));
        }
        // A symlinked entry could escape the plugin directory; resolve it.
        let root = std::fs::canonicalize(dir)
            .map_err(|source| PluginError::io(dir.to_path_buf(), source))?;
        let resolved = std::fs::canonicalize(&absolute)
            .map_err(|source| PluginError::io(absolute.clone(), source))?;
        if !resolved.starts_with(&root) {
            return Err(PluginError::manifest(format!(
                "entry '{}' resolves outside the plugin directory",
                entry.display()
            )));
        }

        let mut capabilities = Vec::with_capacity(meta.capabilities.len());
        for raw in &meta.capabilities {
            let capability = Capability::parse(raw)?;
            if !capabilities.contains(&capability) {
                capabilities.push(capability);
            }
        }
        if capabilities.len() > Capability::ALL.len() {
            return Err(PluginError::manifest("too many capabilities"));
        }
        capabilities.sort();

        if file.settings.len() > MAX_SETTINGS {
            return Err(PluginError::manifest(format!(
                "plugin '{}' declares {} settings (max {MAX_SETTINGS})",
                meta.id,
                file.settings.len()
            )));
        }
        let mut keys = BTreeSet::new();
        for spec in &file.settings {
            spec.validate()?;
            if !keys.insert(spec.key.clone()) {
                return Err(PluginError::manifest(format!(
                    "duplicate setting key '{}'",
                    spec.key
                )));
            }
        }

        Ok(Self {
            id: meta.id,
            name: meta.name.trim().to_string(),
            version: meta.version,
            api: meta.api,
            description: meta.description.trim().to_string(),
            author: meta.author.trim().to_string(),
            homepage: meta.homepage,
            runtime,
            entry,
            capabilities,
            settings: file.settings,
        })
    }

    /// Default configuration for this manifest.
    #[must_use]
    pub fn defaults(&self) -> std::collections::BTreeMap<String, Value> {
        self.settings
            .iter()
            .map(|spec| (spec.key.clone(), spec.value_as_json(&spec.default)))
            .collect()
    }

    /// Merge a stored configuration over the defaults, dropping unknown keys
    /// and falling back to the default for every value that no longer validates
    /// (a plugin update can change its own schema).
    #[must_use]
    pub fn reconcile(&self, stored: &serde_json::Map<String, Value>) -> std::collections::BTreeMap<String, Value> {
        self.settings
            .iter()
            .map(|spec| {
                let value = stored
                    .get(&spec.key)
                    .and_then(|raw| spec.coerce(raw).ok())
                    .unwrap_or_else(|| spec.default.clone());
                (spec.key.clone(), value)
            })
            .collect()
    }

    /// True when the plugin declared a capability.
    #[must_use]
    pub fn allows(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// Raw deserialisation target; validation happens afterwards so that every
/// failure carries a message the settings panel can show verbatim.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    plugin: PluginMeta,
    #[serde(default)]
    settings: Vec<SettingSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginMeta {
    id: String,
    name: String,
    version: String,
    api: u32,
    entry: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

fn runtime_for(entry: &Path) -> Option<RuntimeKind> {
    match entry.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "lua" => Some(RuntimeKind::Lua),
        "wasm" | "wat" => Some(RuntimeKind::Wasm),
        _ => None,
    }
}

fn is_valid_id(id: &str) -> bool {
    if !(3..=64).contains(&id.len()) || !id.contains('.') {
        return false;
    }
    if id.starts_with('.') || id.ends_with('.') || id.contains("..") {
        return false;
    }
    id.chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '_')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

fn is_valid_version(version: &str) -> bool {
    if version.is_empty() || version.len() > 32 {
        return false;
    }
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 6 && part.chars().all(|ch| ch.is_ascii_digit())
        })
}

fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && key.len() <= 32
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

/// Reject absolute paths, parent traversal and Windows prefixes outright: a
/// manifest is data, and data does not get to name `C:\Windows`.
fn validate_relative(entry: &Path) -> Result<(), PluginError> {
    if entry.as_os_str().is_empty() {
        return Err(PluginError::manifest("entry cannot be empty"));
    }
    for component in entry.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PluginError::manifest(format!(
                    "entry '{}' must be a plain relative path",
                    entry.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const VALID: &str = r#"
[plugin]
id = "io.prism.night-mode"
name = "Night Mode"
version = "1.0.0"
api = 1
entry = "main.lua"
description = "Lowers the volume after a given hour."
author = "Prism Contributors"
capabilities = ["log", "notify", "player:read", "player:control"]

[[settings]]
key = "after_hour"
label = "After hour"
type = "integer"
default = 22
min = 0
max = 23

[[settings]]
key = "tone"
label = "Tone"
type = "select"
default = "warm"
options = ["warm", "cool"]
"#;

    fn scratch(entry_name: &str, manifest: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(MANIFEST_FILENAME), manifest).expect("manifest");
        if !entry_name.is_empty() {
            std::fs::write(dir.path().join(entry_name), "-- plugin\n").expect("entry");
        }
        dir
    }

    #[test]
    fn a_valid_manifest_round_trips() {
        let dir = scratch("main.lua", VALID);
        let manifest = Manifest::load(dir.path()).expect("valid manifest");
        assert_eq!(manifest.id, "io.prism.night-mode");
        assert_eq!(manifest.runtime, RuntimeKind::Lua);
        assert!(manifest.allows(Capability::PlayerControl));
        assert_eq!(manifest.settings.len(), 2);
        assert_eq!(manifest.defaults()["after_hour"], json!(22));
        assert_eq!(manifest.defaults()["tone"], json!("warm"));
    }

    #[test]
    fn the_runtime_comes_from_the_entry_extension() {
        let manifest = VALID.replace("main.lua", "main.wasm");
        let dir = scratch("main.wasm", &manifest);
        let parsed = Manifest::load(dir.path()).expect("valid wasm manifest");
        assert_eq!(parsed.runtime, RuntimeKind::Wasm);
    }

    #[test]
    fn an_unknown_extension_is_rejected() {
        let manifest = VALID.replace("main.lua", "main.js");
        let dir = scratch("main.js", &manifest);
        let error = Manifest::load(dir.path()).expect_err("must reject");
        assert!(error.to_string().contains(".lua"), "{error}");
    }

    #[test]
    fn path_traversal_is_rejected_before_touching_the_filesystem() {
        for entry in ["../escape.lua", "/etc/passwd.lua", "sub/../../evil.lua"] {
            let manifest = VALID.replace("main.lua", entry);
            let dir = scratch("", &manifest);
            let error = Manifest::load(dir.path()).expect_err("must reject {entry}");
            assert!(
                error.to_string().contains("relative") || error.to_string().contains("does not exist"),
                "{error}"
            );
        }
    }

    #[test]
    fn an_api_mismatch_is_reported_with_both_numbers() {
        let manifest = VALID.replace("api = 1", "api = 7");
        let dir = scratch("main.lua", &manifest);
        let error = Manifest::load(dir.path()).expect_err("must reject");
        let text = error.to_string();
        assert!(text.contains('7') && text.contains("API 1"), "{text}");
    }

    #[test]
    fn unknown_capabilities_are_named_in_the_error() {
        let manifest = VALID.replace(r#""log", "notify", "player:read", "player:control""#, r#""telepathy""#);
        let dir = scratch("main.lua", &manifest);
        let error = Manifest::load(dir.path()).expect_err("must reject");
        assert!(error.to_string().contains("telepathy"), "{error}");
    }

    #[test]
    fn duplicate_settings_and_bad_keys_are_rejected() {
        let duplicate = format!(
            "{VALID}\n[[settings]]\nkey = \"after_hour\"\nlabel = \"Again\"\ntype = \"integer\"\ndefault = 1\n"
        );
        let dir = scratch("main.lua", &duplicate);
        assert!(Manifest::load(dir.path()).is_err());

        let bad_key = VALID.replace(r#"key = "tone""#, r#"key = "Tone!""#);
        let dir = scratch("main.lua", &bad_key);
        assert!(Manifest::load(dir.path()).is_err());
    }

    #[test]
    fn defaults_are_checked_against_their_own_schema() {
        let out_of_range = VALID.replace("default = 22", "default = 99");
        let dir = scratch("main.lua", &out_of_range);
        let error = Manifest::load(dir.path()).expect_err("must reject");
        assert!(error.to_string().contains("<= 23"), "{error}");

        let bad_select = VALID.replace(r#"default = "warm""#, r#"default = "neon""#);
        let dir = scratch("main.lua", &bad_select);
        assert!(Manifest::load(dir.path()).is_err());
    }

    #[test]
    fn coercion_rejects_rather_than_clamps() {
        let dir = scratch("main.lua", VALID);
        let manifest = Manifest::load(dir.path()).expect("valid");
        let hour = &manifest.settings[0];
        assert_eq!(hour.coerce(&json!(8)).expect("in range"), json!(8));
        assert!(hour.coerce(&json!(24)).is_err());
        assert!(hour.coerce(&json!(-1)).is_err());
        assert!(hour.coerce(&json!(8.5)).is_err());
        assert!(hour.coerce(&json!("8")).is_err());

        let tone = &manifest.settings[1];
        assert!(tone.coerce(&json!("cool")).is_ok());
        assert!(tone.coerce(&json!("neon")).is_err());
    }

    #[test]
    fn reconcile_keeps_valid_values_and_repairs_the_rest() {
        let dir = scratch("main.lua", VALID);
        let manifest = Manifest::load(dir.path()).expect("valid");
        let mut stored = serde_json::Map::new();
        stored.insert("after_hour".into(), json!(7));
        stored.insert("tone".into(), json!("neon")); // no longer an option
        stored.insert("removed_key".into(), json!(true)); // plugin update dropped it

        let config = manifest.reconcile(&stored);
        assert_eq!(config["after_hour"], json!(7));
        assert_eq!(config["tone"], json!("warm"));
        assert!(!config.contains_key("removed_key"));
    }

    #[test]
    fn identifiers_and_versions_are_strict() {
        assert!(is_valid_id("io.prism.player"));
        assert!(!is_valid_id("nodot"));
        assert!(!is_valid_id("IO.PRISM.PLAYER"));
        assert!(!is_valid_id("-leading.dash"));
        assert!(is_valid_version("1.0.0"));
        assert!(is_valid_version("10.20.30-beta.1"));
        assert!(!is_valid_version("1.0"));
        assert!(!is_valid_version("v1.0.0"));
    }

    #[test]
    fn unknown_manifest_keys_are_rejected() {
        let manifest = format!("{VALID}\n[surprise]\nvalue = 1\n");
        let dir = scratch("main.lua", &manifest);
        let error = Manifest::load(dir.path()).expect_err("must reject");
        assert!(error.to_string().contains("surprise") || error.to_string().contains("unknown"), "{error}");
    }
}
