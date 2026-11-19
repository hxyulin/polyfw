# polyfw

Language for the relationship between the installed CLI and a consuming project's configuration.

## Language

**Configuration compatibility**:
The ability of a particular polyfw CLI release to interpret a `polyfw.toml` configuration
according to its configuration contract. Compatibility is directional: a newer CLI may
understand an older configuration without the older CLI understanding a newer configuration.
_Avoid_: Version equality

**Configuration contract version**:
The polyfw release whose configuration contract a project configuration follows.
It identifies the expected structure and meaning of the configuration, rather than the last
CLI release that happened to read it.
_Avoid_: Generator version, project version

**Minimum required CLI version**:
The oldest polyfw release expected to interpret and execute a particular project
configuration according to its configuration contract.

**Oldest supported configuration version**:
The earliest configuration contract a particular polyfw CLI release accepts.
It marks that release's retained configuration history, independently of a configuration's
minimum required CLI version.
