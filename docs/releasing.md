# Releasing polyfw

The maintainer prepares a version on `main`, then pushes a matching tag such as `v0.2.0` or
`v0.2.0-rc.1`. The tag starts `.github/workflows/release.yml`:

1. Validate SemVer, the Cargo version, the triggering commit and its ancestry on `main`.
2. Run reusable CI, verify the packaged crate, and build and smoke-test all five binary targets.
3. Create a GitHub draft with the binary archives and `SHA256SUMS`.
4. Publish the crate using OIDC, or verify and reuse an identical existing crate version.
5. Recheck the tag, crate and assets, then publish the GitHub Release.

The initial crates.io publication is performed locally, as described below. Subsequent stable and
prerelease versions use the same automatic workflow. A tag identifies one source commit throughout
the process; keep it unchanged when retrying.

## One-time setup and the first crate

The crate must already exist before its owner can configure
[crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing).
The first publish therefore uses a local API token. This setup does not require a long-lived
crates.io token in GitHub secrets.

1. Merge the release workflow and the prepared first version into `main`. Use a clean checkout
   of that exact commit, with its `Cargo.lock` and pinned Rust compiler. Complete the version
   preparation checks below before publishing anything.
2. Sign in to crates.io with GitHub and verify the account email. Create a short-lived API token
   with permission to publish a new crate. Check that the `polyfw` name is still available.
3. From the repository root, authenticate interactively and publish:

   ```sh
   cargo login
   cargo publish --dry-run --locked
   cargo publish --locked
   cargo logout
   ```

   Cargo prompts for the token; keep it out of shell command history. Revoke the bootstrap token
   after this publication. Wait for the new version to appear on crates.io.
4. As a crate owner, open its crates.io settings and add a GitHub Actions trusted publisher:

   | Field | Value |
   | --- | --- |
   | Repository owner | `inaku-Gyan` |
   | Repository name | `polyfw` |
   | Workflow filename | `release.yml` |
   | Environment | Leave empty; the publishing job does not use an environment |

   The publishing job grants `id-token: write` and exchanges GitHub's identity for a short-lived
   registry token with `rust-lang/crates-io-auth-action`. Keep this registration aligned with any
   future repository or workflow rename.
5. Push the first version's tag using the steps below. The workflow packages the same committed
   source, compares its SHA256 with the existing crate, skips the registry upload and creates
   the GitHub Release.

Publish locally **before** pushing the first tag, from the same clean source commit. The workflow
checks the entire `.crate` checksum, including its Cargo-generated VCS metadata. A publication
from a dirty checkout or different commit will not be accepted as the same release. Published
crate versions are immutable; a conflicting version needs a new version number.

## Prepare each version

Use the repository's pinned toolchain. Maintainer checks need `just`, Node.js 18 or newer for the
release tests, and the embedded target used by the Rust test fixtures:

```sh
rustup target add thumbv7em-none-eabihf
```

1. Change `[package].version` in `Cargo.toml`, then run `cargo check` to update the root package
   entry in `Cargo.lock`. Review the diff; an ordinary version bump does not need dependency updates.
2. Run the checks and inspect the crate file list:

   ```sh
   just check
   node --test .github/scripts/release.test.mjs
   cargo package --list --locked --allow-dirty
   cargo publish --dry-run --locked --allow-dirty
   git diff --check
   ```

   `--allow-dirty` is useful while reviewing uncommitted version changes. The actual publication
   uses a clean committed tree and omits this flag. The dry run builds the extracted crate without
   uploading it. Release automation and maintainer docs are excluded from the crate; the embedded
   schema, license, README, source and Rust tests remain included.
3. Commit `Cargo.toml`, `Cargo.lock` and any accompanying documentation. Merge the preparation
   commit into `main`, through the normal review process.

The Rust minimum in `Cargo.toml` and the development compiler in `rust-toolchain.toml` are separate
settings. Assess them when changing Rust features or dependencies; an ordinary package version
bump does not require changing either.

## Push the release tag

Start from the clean, merged release commit. Substitute the actual prepared version:

```sh
git switch main
git pull --ff-only
git status --short
cargo publish --dry-run --locked

git tag -a v0.2.0 -m 'Release v0.2.0'
git push origin v0.2.0
```

`git status --short` should print nothing. Push only the intended release tag. The workflow checks
that the version matches `Cargo.toml`, the tag still points at the triggering commit, and that
commit is reachable from `origin/main`. Prereleases use the same rules and can use an annotated
tag such as `v0.2.0-rc.1` after preparing `version = "0.2.0-rc.1"`.

The workflow reads the pinned compiler rather than following floating stable Rust. Same-tag runs
are serialized and do not cancel the active run. Track the `Release` workflow in GitHub Actions;
after it succeeds, verify both the GitHub Release and the crates.io version.

## Platforms and installation

| Target | Build and smoke-test environment | Archive |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Native x86_64 manylinux2014 container | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Native ARM64 manylinux2014 container | `.tar.gz` |
| `x86_64-apple-darwin` | Native Intel macOS runner | `.tar.gz` |
| `aarch64-apple-darwin` | Native ARM64 macOS runner | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Native x86_64 Windows runner | `.zip` |

Linux containers retain the reference project's glibc 2.17 baseline. JavaScript actions run on
the newer Ubuntu host, outside the container. Every binary must report the expected `--version`,
provide `--help`, and emit valid JSON from `schema`. These checks do not require firmware projects
or board hardware. The full Rust test suite runs on Linux in the reusable CI job.

Assets are named `polyfw-<target>.tar.gz` or `polyfw-<target>.zip`; the version is selected by the
GitHub tag. Each archive contains `polyfw-<target>/` with `polyfw` (or `polyfw.exe`), `README.md`
and `LICENSE`. `Cargo.toml` supplies cargo-binstall with this URL and executable layout.

Download `SHA256SUMS` alongside the desired archive and check its entry before extracting. On
Linux use `sha256sum --ignore-missing --check SHA256SUMS`; on macOS use `shasum -a 256 <archive>`;
on Windows use `Get-FileHash <archive> -Algorithm SHA256` and compare with `SHA256SUMS`.
The checksum file is for explicit download verification; binstall metadata describes the
download URL and archive layout.

Published stable versions can be installed with:

```sh
cargo binstall polyfw
# Or build from crates.io:
cargo install polyfw --locked
```

Prereleases are published to both destinations. GitHub marks them as prereleases and never makes
them `latest`. Unversioned Cargo installation selects stable versions; request a candidate explicitly:

```sh
cargo binstall polyfw --version 0.2.0-rc.1
cargo install polyfw --locked --version 0.2.0-rc.1
```

GitHub generates release notes. A stable version compares against the highest earlier stable
release, including all changes made during its prerelease cycle. A prerelease compares against
the previous prerelease of the same major/minor/patch series, falling back to the previous stable
release for the first candidate. An older stable release does not replace a newer stable `latest`.

## Recover a failed run

GitHub and crates.io do not publish atomically. The draft keeps GitHub assets private until the
crate is available, and the workflow records the source commit and crate checksum in a hidden
release-note marker. Preserve that marker and the generated assets when rerunning a release.

| Failure point | Recovery |
| --- | --- |
| CI, packaging or binary build | Fix a transient runner/network issue and rerun the failed jobs. No publication has happened yet. Source fixes need a new commit, version and tag. |
| Partial draft upload | Rerun failed jobs for the same tag. Matching draft assets are reused; incomplete or changed draft assets can be replaced. |
| OIDC authentication | Check the trusted publisher's owner, repository, workflow filename and empty environment, then rerun. The GitHub Release remains a draft. |
| Crate upload or index polling timeout | Check crates.io first. An upload can succeed before a polling timeout. Rerunning verifies the registry checksum and skips an identical existing version. |
| GitHub publication after the crate succeeded | Rerun failed jobs. The final job rechecks the existing crate and assets, then publishes the existing draft. |
| Rerun after full success | The workflow verifies and reuses the existing crate and public assets. Rebuilt archive bytes are not substituted into a public release. |
| Version exists with a different checksum, tag moved, or public assets conflict | Stop and investigate the source mismatch. Prepare a new version for changed source; do not overwrite the published version or move its tag. |

Use GitHub Actions **Re-run failed jobs**, or `gh run rerun <run-id> --failed`. Re-running all jobs
is also supported, including after the first local crate publication. If a draft upload must be
repeated, the draft stays private until its complete checksum manifest has been verified.

For a source or workflow correction, merge the fix into `main` and prepare the next version.
Rerunning an old tag intentionally uses the old tagged workflow and source. Registry yanking and
release withdrawal are separate maintainer actions; consult the
[Cargo publishing documentation](https://doc.rust-lang.org/cargo/reference/publishing.html)
before withdrawing an already consumed version.
