---
status: accepted
---

# Release-linked configuration compatibility

polyfw needs directional configuration compatibility without a promise to preserve every old
format. Use CLI release versions for the configuration contract and separate the CLI's oldest
supported configuration from each file's minimum required CLI, keeping the relevant releases
visible to users. Preserve old configurations only while the current implementation handles
their original meaning; otherwise raise the compatibility floor and require manual migration.

Independent format numbers would require users to look up the associated CLI release, while
exact CLI-version equality would force configuration updates for compatible releases. The
chosen policy accepts the maintenance cost of explicit compatibility declarations and tests,
with no migration adapters, automatic migration or bypass.

The [configuration guide](../configuration.md#configuration-compatibility) owns the public
contract; [Design](../design.md#configuration-compatibility) describes the loading boundary
and implementation policy.
