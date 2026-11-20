use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::Path;

use crate::Result;
use crate::config_version::{CLI_VERSION, MIN_REQUIRED_CLI_VERSION};
use crate::ui::Ui;

pub(crate) fn run(root: &Path, ui: &Ui) -> Result<()> {
    let path = root.join("polyfw.toml");
    // Exclusive creation refuses existing files, directories and dangling symlinks.
    let mut file = File::create_new(&path).map_err(|error| {
        if error.kind() == ErrorKind::AlreadyExists {
            format!(
                "{} already exists; move or remove it before running polyfw init",
                path.display()
            )
        } else {
            format!("could not create {}: {error}", path.display())
        }
    })?;
    let result = file.write_all(template().as_bytes());
    drop(file);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&path);
        return Err(format!("could not write {}: {error}", path.display()).into());
    }
    ui.initialized(&path);
    Ok(())
}

fn template() -> String {
    let source = format!(
        include_str!("../assets/polyfw.toml"),
        CLI_VERSION = CLI_VERSION,
        MIN_REQUIRED_CLI_VERSION = MIN_REQUIRED_CLI_VERSION,
    );
    let (header, body) = source
        .split_once('\n')
        .expect("initialization template must contain a schema header and a body");
    assert!(
        header.starts_with("#:schema "),
        "initialization template must start with a schema directive"
    );
    format!(
        "#:schema https://raw.githubusercontent.com/inaku-Gyan/polyfw/v{CLI_VERSION}/assets/polyfw.schema.json\n{body}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_version;
    use crate::model::Registry;

    #[test]
    fn the_template_uses_the_release_contract_and_authoring_minimum() {
        let source = template();
        config_version::preflight(&source, Path::new("polyfw.toml")).unwrap();
        let registry: Registry = toml::from_str(&source).unwrap();
        assert_eq!(registry.config_version, CLI_VERSION);
        assert_eq!(registry.min_polyfw_version, MIN_REQUIRED_CLI_VERSION);

        let schema: serde_json::Value =
            serde_json::from_str(&crate::registry::schema_source().unwrap()).unwrap();
        assert_eq!(
            source.lines().next().unwrap().strip_prefix("#:schema "),
            schema["$id"].as_str()
        );
    }
}
