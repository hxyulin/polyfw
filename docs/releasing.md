# Releasing

This runbook covers stable and SemVer prerelease versions on GitHub Releases and crates.io.
Prepare a version on `main`, then push its matching tag, such as `v0.2.0` or `v0.2.0-rc.1`.
GitHub Actions publishes the crate through OIDC and completes the GitHub Release.

## Publication contract

[The release workflow](../.github/workflows/release.yml) runs these stages:

1. Validate the tag's SemVer, its match with `Cargo.toml`, the triggering source commit and that
   commit's ancestry on `main`.
2. Run reusable CI, verify the packaged crate, and build/smoke-test all five binary targets.
3. Upload archives and `SHA256SUMS` to a GitHub draft release.
4. Publish to crates.io through OIDC, or verify an identical version already published there.
5. Recheck the tag, crate checksum and assets, then make the GitHub Release public.

A release identifies one source commit and one complete `.crate` checksum. Keep the tag unchanged
when retrying. The workflow reads the repository's pinned Rust compiler; same-tag runs are
serialized without cancelling an active run.

Prereleases go to both channels. GitHub marks them as prereleases and never sets them as `latest`.
A stable release becomes `latest` unless a newer stable release is already public.

## Configure the publisher

In the crate's crates.io settings, register a GitHub Actions
[trusted publisher](https://crates.io/docs/trusted-publishing) matching the publishing job:

| Field | Value |
| --- | --- |
| Repository owner | `inaku-Gyan` |
| Repository name | `polyfw` |
| Workflow filename | `release.yml` |
| Environment | Leave empty; the publishing job declares no environment |

The job grants `id-token: write` and uses `rust-lang/crates-io-auth-action` to exchange GitHub's
identity for a short-lived registry token. Keep the registration aligned with any repository,
workflow or environment change. No long-lived crates.io token is needed in GitHub secrets.

## Prepare a version

Use the development setup and checks in [Development](development.md). In particular, Rust test
fixtures need `thumbv7em-none-eabihf`, and release tests need Node.js 18 or newer.

1. Update `[package].version` in `Cargo.toml`. Use a full stable version or a SemVer prerelease,
   for example `0.2.0-rc.1` or `0.2.0-beta.1`.
2. Run `cargo check` to update the root package entry in `Cargo.lock`. Review that diff; an
   ordinary version bump does not require dependency upgrades or a new Rust pin.
3. Run the checks and inspect the published package contents:

   ```sh
   just check
   node --test .github/scripts/release.test.mjs
   cargo package --list --locked --allow-dirty
   cargo publish --dry-run --locked --allow-dirty
   git diff --check
   ```

   `--allow-dirty` permits reviewing uncommitted preparation changes. The dry run builds the
   extracted crate without uploading. The actual publication uses a clean committed tree.
   Package exclusions in `Cargo.toml` keep agent instructions, detailed guides, release automation
   and local distribution output out of the crate. Source, README, license, embedded schema and
   Rust tests remain included.
4. Commit the manifest, lockfile and accompanying changes, then merge them into `main` through
   the normal review process.

## Push the tag

Complete version preparation and confirm the trusted publisher is configured. Start from the
clean, merged commit with its pinned Rust compiler and lockfile, and substitute the actual version:

```sh
git switch main
git pull --ff-only
git status --short
cargo publish --dry-run --locked

git tag -a v0.2.0 -m 'Release v0.2.0'
git push origin v0.2.0
```

`git status --short` must print nothing. For a candidate, prepare `version = "0.2.0-rc.1"` and
use `v0.2.0-rc.1` in both tag commands. Push only the intended tag.

The crate checksum covers the complete archive, including Cargo-generated VCS metadata. Reruns
must use the same source commit; an existing version with different contents requires a new version.

The workflow requires the tag version to equal the package version, the live tag to resolve to
the triggering commit, and that commit to be reachable from `origin/main`. Track the `Release`
run in GitHub Actions. After success, check the public GitHub assets and crates.io version;
installation commands are in [README](../README.md#install).

## Assets and build targets

| Target | Build and smoke-test environment | Archive |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Native x86_64 manylinux2014 container | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Native ARM64 manylinux2014 container | `.tar.gz` |
| `x86_64-apple-darwin` | Native Intel macOS runner | `.tar.gz` |
| `aarch64-apple-darwin` | Native ARM64 macOS runner | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Native x86_64 Windows runner | `.zip` |

Linux containers preserve a glibc 2.17 baseline; JavaScript actions run on the newer Ubuntu host
outside the container. Each built executable must report the expected `--version`, provide
`--help` and emit valid JSON for `schema`. These smoke tests need no firmware project or hardware.
The full Rust suite runs on Linux through reusable CI; the five-platform binary matrix does not
claim physical probe/debugger validation.

Archives are named `polyfw-<target>.tar.gz` or `polyfw-<target>.zip`. Each contains a directory
`polyfw-<target>/` holding `polyfw` (Windows: `polyfw.exe`), `README.md` and `LICENSE`.
The tag selects the version. Binstall metadata in `Cargo.toml` specifies this URL and executable
layout. Detailed guides and the schema file are not bundled in the binary archive; README links
to online guides, and the executable can generate its matching schema.

Download `SHA256SUMS` beside the desired archive, then verify before extracting:

| Host | Check |
| --- | --- |
| Linux | `sha256sum --ignore-missing --check SHA256SUMS` |
| macOS | `shasum -a 256 <archive>` and compare its result with the manifest entry |
| Windows PowerShell | `Get-FileHash <archive> -Algorithm SHA256` and compare with the manifest entry |

The checksum manifest supports explicit downloads; the binstall metadata provides the asset URL
and layout. The workflow verifies uploaded archive bytes against the manifest before publication.

## Release notes

GitHub generates notes automatically. The comparison base is:

- For a stable release: the highest earlier stable release, including changes across its
  prerelease cycle.
- For a prerelease: the preceding prerelease of the same major/minor/patch series, falling back
  to the earlier stable release for the first candidate.

The release body also carries a hidden source-commit and crate-checksum marker. Preserve it and
the generated assets when retrying; the scripts use it to establish release identity.

## Recover a failed run

GitHub and crates.io publish separately. Keeping GitHub as a draft until the crate is available
lets a rerun finish the same version without exposing a release whose crate is missing.

| Failure point | Recovery |
| --- | --- |
| CI, packaging or binary build | Resolve a transient runner/network problem and rerun failed jobs. Source fixes require a new commit, version and tag. |
| Partial draft upload | Rerun the same tag. Matching assets are reused; incomplete or changed draft assets can be replaced. |
| OIDC authentication | Check the registered owner, repository, workflow filename and empty environment, then rerun. GitHub stays a draft. |
| Crate upload or availability timeout | Inspect crates.io first: upload can succeed before polling times out. A rerun verifies the checksum and skips an identical existing version. |
| GitHub publication after crate success | Rerun failed jobs. The final job verifies the crate and assets, then publishes the existing draft. |
| Rerun after full success | Existing crate and public assets are verified and reused. Rebuilt archives do not replace public assets. |
| Existing crate checksum differs, tag moved, or public assets conflict | Investigate the mismatch. Changed source needs a new version; preserve the published version and tag. |

Use GitHub Actions **Re-run failed jobs**, or `gh run rerun <run-id> --failed`. Re-running all jobs
is also supported.

A rerun uses the old tag's workflow and source. For a source or workflow correction, merge the
fix into `main` and prepare the next version. Registry yanking or withdrawing a release is a
separate maintainer action; consult [Cargo publishing](https://doc.rust-lang.org/cargo/reference/publishing.html)
for the registry procedure.
