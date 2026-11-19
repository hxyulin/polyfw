use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::profile::Profile;

const REGISTRY_NAME_PATTERN: &str = "^[a-z][a-z0-9]*(-[a-z0-9]+)*$";

/// Project configuration for registered embedded firmware.
/// Cargo metadata additionally validates BSP workspaces and firmware packages at runtime.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "polyfw registry", extend(
    "$id" = concat!(
        "https://raw.githubusercontent.com/inaku-Gyan/polyfw/v",
        env!("CARGO_PKG_VERSION"),
        "/polyfw.schema.json"
    )
))]
pub(crate) struct Registry {
    /// Optional tool settings; omitted values keep their defaults.
    #[serde(default)]
    pub(crate) config: Config,

    /// Board models and their consuming workspaces, keyed by BSP ID such as controller-model.
    #[schemars(extend("minProperties" = 1, "propertyNames" = {
        "type": "string",
        "description": "ID starting with a lowercase letter, followed by lowercase letters, digits or single hyphens.",
        "pattern": REGISTRY_NAME_PATTERN,
    }))]
    pub(crate) bsps: BTreeMap<String, Bsp>,
    /// Programmable targets, keyed by board ID such as controller.
    #[schemars(extend("minProperties" = 1, "propertyNames" = {
        "type": "string",
        "description": "ID starting with a lowercase letter, followed by lowercase letters, digits or single hyphens.",
        "pattern": REGISTRY_NAME_PATTERN,
    }))]
    pub(crate) boards: BTreeMap<String, Board>,
}

/// Project-wide checking and editor settings.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    /// Directory globs selecting reachable path packages for fmt, lint, test, verify
    /// and detailed info. Defaults to ["./**"]; [] selects no dependency packages.
    /// The firmware itself is always included; registry and Git dependencies are excluded.
    ///
    /// Match each package's canonical Cargo.toml directory. Relative patterns start
    /// beside polyfw.toml; absolute paths and leading `..` can include external packages.
    /// `shared` matches only that directory, `shared/*` its immediate children, and
    /// `shared/**` the directory and all descendants. Literal prefixes are canonicalized;
    /// `..` must precede wildcards. Unmatched valid patterns select nothing; invalid
    /// patterns are errors. These globs select checks, not Cargo's dependency builds.
    #[serde(rename = "local-dependencies")]
    pub(crate) local_dependencies: Vec<String>,
    /// Editor-file management when saving or clearing board selection.
    pub(crate) vscode: VscodeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            local_dependencies: vec!["./**".into()],
            vscode: VscodeConfig::default(),
        }
    }
}

/// VS Code launch-file generation settings.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct VscodeConfig {
    /// Replace the entire .vscode/launch.json on selection save and delete it on clear.
    /// Defaults to true. Set false to leave editor files untouched, including existing
    /// files generated while this option was enabled.
    pub(crate) enabled: bool,
}

impl Default for VscodeConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// A board model's consuming Cargo workspace and hardware identity.
#[derive(Debug, Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bsp {
    /// Cargo workspace directory relative to the project root. Use `.` for the root;
    /// absolute paths and `..` are rejected. The workspace can be virtual.
    pub(crate) workspace: String,
    /// Resolved target directory name; derived from Cargo, not read from polyfw.toml.
    #[serde(skip)]
    pub(crate) target: String,
    /// Last explicit target CPU in effective rustflags, or None for the compiler default.
    #[serde(skip)]
    pub(crate) cpu: Option<String>,
    /// probe-rs chip name for hardware commands and editor sessions, for example STM32H723VGTx.
    pub(crate) chip: String,
}

/// A programmable target referring to a BSP and firmware package.
#[derive(Debug, Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Board {
    /// Human-readable label for board lists and the picker.
    pub(crate) description: String,
    /// BSP ID declared in this registry's bsps table.
    pub(crate) bsp: String,
    /// Cargo package name in the BSP workspace. It must declare a host-testable library
    /// and at least one firmware binary.
    pub(crate) package: String,
    /// Firmware binary target name. Omit only if the package declares exactly one binary,
    /// counting feature-gated binaries independently of Cargo's default-run setting.
    pub(crate) bin: Option<String>,
    #[serde(skip)]
    pub(crate) resolved_bin: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Metadata {
    pub(crate) workspace_root: String,
    pub(crate) workspace_members: Vec<String>,
    pub(crate) packages: Vec<MetadataPackage>,
    pub(crate) resolve: Option<MetadataResolve>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetadataPackage {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) source: Option<String>,
    pub(crate) manifest_path: PathBuf,
    pub(crate) targets: Vec<MetadataTarget>,
}

impl Metadata {
    pub(crate) fn workspace_package(&self, name: &str) -> Option<&MetadataPackage> {
        self.packages
            .iter()
            .find(|package| package.name == name && self.workspace_members.contains(&package.id))
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetadataResolve {
    pub(crate) nodes: Vec<MetadataNode>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetadataNode {
    pub(crate) id: String,
    pub(crate) deps: Vec<MetadataDependency>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetadataDependency {
    pub(crate) pkg: String,
    pub(crate) dep_kinds: Vec<MetadataDependencyKind>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetadataDependencyKind {
    pub(crate) kind: Option<DependencyKind>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DependencyKind {
    Build,
    Dev,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetadataTarget {
    pub(crate) name: String,
    pub(crate) kind: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct Selection {
    pub(crate) id: String,
    pub(crate) board: Board,
    pub(crate) bsp: Bsp,
    pub(crate) profile: Option<Profile>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedSelection {
    pub(crate) board: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) profile: Option<Profile>,
}
