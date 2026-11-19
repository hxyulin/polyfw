use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::profile::Profile;

const REGISTRY_NAME_PATTERN: &str = "^[a-z][a-z0-9]*(-[a-z0-9]+)*$";

/// BSPs and the boards built from them. polyfw also checks each workspace and package through
/// Cargo metadata.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "polyfw registry", extend(
    "$id" = "https://raw.githubusercontent.com/inaku-Gyan/polyfw/main/polyfw.schema.json"
))]
pub(crate) struct Registry {
    /// Project-wide tool options. BSPs and boards describe the firmware separately.
    #[serde(default)]
    pub(crate) config: Config,

    /// Board support packages, keyed by BSP ID, such as dm-mc02.
    #[schemars(extend("minProperties" = 1, "propertyNames" = {
        "type": "string",
        "description": "Lowercase letters, digits and single hyphens, starting with a letter.",
        "pattern": REGISTRY_NAME_PATTERN,
    }))]
    pub(crate) bsps: BTreeMap<String, Bsp>,
    /// Programmable boards, keyed by board ID, such as sentry-chassis.
    #[schemars(extend("minProperties" = 1, "propertyNames" = {
        "type": "string",
        "description": "Lowercase letters, digits and single hyphens, starting with a letter.",
        "pattern": REGISTRY_NAME_PATTERN,
    }))]
    pub(crate) boards: BTreeMap<String, Board>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    /// Directory globs selecting reachable Cargo path dependencies for fmt, lint, test, verify
    /// and detailed info. Relative patterns start at the directory holding polyfw.toml;
    /// absolute paths and leading `..` may include external packages. Match each package's
    /// canonical Cargo.toml parent directory: `shared` selects only shared/Cargo.toml,
    /// `shared/*` selects immediate package directories, and `shared/**` includes shared and all
    /// descendants. Existing literal directory prefixes are canonicalized. Selected firmware
    /// is always included; registry and Git packages never are.
    /// Omission defaults to ["./**"]. An empty array selects no dependencies, valid unmatched
    /// patterns select nothing, and invalid globs are errors. `..` must precede any wildcard.
    #[serde(rename = "local-dependencies")]
    pub(crate) local_dependencies: Vec<String>,
    /// VS Code launch generation when a selection is saved or cleared.
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

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct VscodeConfig {
    /// Manage the entire .vscode/launch.json when saving or clearing a selection.
    /// Defaults to true. When false, leave any existing launch.json untouched.
    pub(crate) enabled: bool,
}

impl Default for VscodeConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bsp {
    /// Cargo workspace directory, relative to the operating project root, without `..`.
    pub(crate) workspace: String,
    /// Target directory name resolved from the consuming workspace's Cargo configuration.
    #[serde(skip)]
    pub(crate) target: String,
    /// Last explicit target CPU in the resolved rustflags, or the compiler's default.
    #[serde(skip)]
    pub(crate) cpu: Option<String>,
    /// probe-rs chip name for hardware commands and VS Code, such as STM32H723VGTx.
    pub(crate) chip: String,
}

#[derive(Debug, Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Board {
    /// Name shown in board lists and prompts.
    pub(crate) description: String,
    /// ID of a BSP in this registry.
    pub(crate) bsp: String,
    /// Firmware package in the consuming workspace. It must have a library target.
    pub(crate) package: String,
    /// Firmware binary target; omit only when the package declares exactly one binary.
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
