use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serializer};

/// A Cargo profile name, such as `dev`, `release` or a custom profile.
///
/// Parse one with [`str::parse`]. Names contain only ASCII letters, digits, `_` and `-`,
/// so a profile directory cannot leave the target directory.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Profile(String);

impl Profile {
    /// The profile name passed to `cargo --profile`.
    pub fn name(&self) -> &str {
        &self.0
    }

    // Cargo shares output directories for its built-in build and test profiles.
    // https://doc.rust-lang.org/cargo/reference/build-cache.html
    pub(crate) fn directory(&self) -> &str {
        match self.name() {
            "dev" | "test" => "debug",
            "release" | "bench" => "release",
            name => name,
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The error for a profile name with characters other than ASCII letters, digits, `_` or `-`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseProfileError(());

impl fmt::Display for ParseProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("profile names must contain only letters, digits, '_' or '-'")
    }
}

impl std::error::Error for ParseProfileError {}

impl FromStr for Profile {
    type Err = ParseProfileError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        name.to_owned().try_into()
    }
}

impl TryFrom<String> for Profile {
    type Error = ParseProfileError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(ParseProfileError(()));
        }
        Ok(Self(name))
    }
}

// State files and reports store a profile as its name. These helpers keep that form private,
// so the public type makes no serialization promise.
pub(crate) fn serialize<S: Serializer>(
    profile: &Profile,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(profile.name())
}

pub(crate) mod optional {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(
        profile: &Option<Profile>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match profile {
            Some(profile) => serializer.serialize_some(profile.name()),
            None => serializer.serialize_none(),
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Profile>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(Profile::try_from)
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_share_cargos_directories_and_custom_profiles_keep_their_names() {
        for (name, directory) in [
            ("dev", "debug"),
            ("test", "debug"),
            ("release", "release"),
            ("bench", "release"),
            ("bench-debug", "bench-debug"),
        ] {
            let profile: Profile = name.parse().unwrap();
            assert_eq!(profile.directory(), directory);
        }
    }

    #[test]
    fn a_profile_cannot_escape_its_firmware_output_directory() {
        for name in ["", ".", "..", "../release", "/tmp/image", "a\\b", "a b"] {
            assert!(name.parse::<Profile>().is_err(), "{name:?}");
        }
        for name in ["dev", "release", "debug_2", "test-fast"] {
            assert!(name.parse::<Profile>().is_ok(), "{name:?}");
        }
    }
}
