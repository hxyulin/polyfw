import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import * as fs from 'node:fs';
import path from 'node:path';

export const targets = [
  { target: 'x86_64-unknown-linux-gnu', runner: 'ubuntu-24.04', image: 'quay.io/pypa/manylinux2014_x86_64' },
  { target: 'aarch64-unknown-linux-gnu', runner: 'ubuntu-24.04-arm', image: 'quay.io/pypa/manylinux2014_aarch64' },
  { target: 'x86_64-apple-darwin', runner: 'macos-15-intel', image: '' },
  { target: 'aarch64-apple-darwin', runner: 'macos-15', image: '' },
  { target: 'x86_64-pc-windows-msvc', runner: 'windows-2022', image: '' },
];

const identifier = '(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)';
const semver = new RegExp(`^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(?:-(${identifier}(?:\\.${identifier})*))?(?:\\+([0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*))?$`);

export function parseVersion(version) {
  const match = semver.exec(version);
  assert(match, `Invalid SemVer: ${version}`);
  return { core: match.slice(1, 4), prerelease: match[4]?.split('.') ?? [] };
}

export function compareVersions(left, right) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  const compare = (x, y) => x < y ? -1 : x > y ? 1 : 0;
  for (let i = 0; i < 3; i++) {
    const result = compare(BigInt(a.core[i]), BigInt(b.core[i]));
    if (result) return result;
  }
  if (!a.prerelease.length || !b.prerelease.length) {
    return compare(!a.prerelease.length, !b.prerelease.length);
  }
  for (let i = 0; i < Math.max(a.prerelease.length, b.prerelease.length); i++) {
    const x = a.prerelease[i];
    const y = b.prerelease[i];
    if (x === undefined || y === undefined) return compare(x !== undefined, y !== undefined);
    const numericX = /^[0-9]+$/.test(x);
    const numericY = /^[0-9]+$/.test(y);
    const result = numericX && numericY ? compare(BigInt(x), BigInt(y))
      : numericX !== numericY ? compare(!numericX, !numericY) : compare(x, y);
    if (result) return result;
  }
  return 0;
}

export function notesBase(releases, version) {
  const current = parseVersion(version);
  const earlier = releases.filter(release => {
    if (release.draft || !release.tag_name.startsWith('v')) return false;
    try { return compareVersions(release.tag_name.slice(1), version) < 0; }
    catch { return false; }
  });
  const stable = earlier.filter(release => !parseVersion(release.tag_name.slice(1)).prerelease.length);
  const sameSeries = earlier.filter(release => {
    const parsed = parseVersion(release.tag_name.slice(1));
    return parsed.prerelease.length && parsed.core.join('.') === current.core.join('.');
  });
  const candidates = current.prerelease.length && sameSeries.length ? sameSeries : stable;
  return candidates.sort((a, b) => compareVersions(b.tag_name.slice(1), a.tag_name.slice(1)))[0]?.tag_name;
}

const command = (program, args, cwd) => execFileSync(program, args, { cwd, encoding: 'utf8', stdio: 'pipe' }).trim();
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
export const archiveName = target => `polyfw-${target}.${target.includes('windows') ? 'zip' : 'tar.gz'}`;
const archiveNames = () => targets.map(({ target }) => archiveName(target)).sort();

function readSchema(root, version) {
  const schema = JSON.parse(fs.readFileSync(path.join(root, 'polyfw.schema.json'), 'utf8'));
  assert.equal(schema.$id, `https://raw.githubusercontent.com/inaku-Gyan/polyfw/v${version}/polyfw.schema.json`,
    'Schema $id differs from the package version; run just schema');
  return schema;
}

export function validateTag(root, tag, expectedCommit) {
  root = fs.realpathSync(root);
  assert(tag.startsWith('v'), 'Release tags must start with v');
  const version = tag.slice(1);
  const parsed = parseVersion(version);
  const metadata = JSON.parse(command('cargo', ['metadata', '--no-deps', '--locked', '--format-version', '1'], root));
  const pkg = metadata.packages.find(pkg => path.resolve(pkg.manifest_path) === path.join(root, 'Cargo.toml'));
  assert.equal(pkg?.name, 'polyfw', 'Expected the root polyfw package');
  assert.equal(version, pkg.version, 'Tag and Cargo.toml versions differ');
  readSchema(root, version);
  const commit = command('git', ['rev-parse', 'HEAD'], root);
  let triggeringCommit;
  try {
    triggeringCommit = command('git', ['rev-parse', `${expectedCommit}^{commit}`], root);
  } catch (error) {
    throw new Error('Cannot resolve the triggering commit', { cause: error });
  }
  assert.equal(commit, triggeringCommit, 'Checkout differs from the triggering commit');
  assert.equal(command('git', ['rev-parse', `refs/tags/${tag}^{commit}`], root), commit, 'Tag has moved');
  try {
    execFileSync('git', ['merge-base', '--is-ancestor', commit, 'refs/remotes/origin/main'], { cwd: root });
  } catch (error) {
    throw new Error('Tagged commit must be merged into main', { cause: error });
  }
  const rustToolchain = /^rustc ([0-9]+\.[0-9]+\.[0-9]+)\b/.exec(command('rustc', ['--version'], root))?.[1];
  assert(rustToolchain, 'Expected a versioned stable Rust toolchain');
  return { version, commit, prerelease: !!parsed.prerelease.length, rust_toolchain: rustToolchain, matrix: { include: targets } };
}

export function packageBinary(root, target, version) {
  assert(targets.some(entry => entry.target === target), `Unsupported target: ${target}`);
  parseVersion(version);
  const binaryName = `polyfw${target.includes('windows') ? '.exe' : ''}`;
  const binary = path.join(root, 'target', target, 'release', binaryName);
  assert.equal(command(binary, ['--version'], root), `polyfw ${version}`, 'Binary version differs from the tag');
  assert.match(command(binary, ['--help'], root), /Usage:/);
  assert.deepEqual(JSON.parse(command(binary, ['schema'], root)), readSchema(root, version),
    'Binary schema differs from committed polyfw.schema.json');
  const dist = path.join(root, 'dist');
  const stage = path.join(dist, `polyfw-${target}`);
  fs.mkdirSync(stage, { recursive: true });
  for (const [source, name] of [[binary, binaryName], [path.join(root, 'README.md'), 'README.md'], [path.join(root, 'LICENSE'), 'LICENSE']]) {
    fs.copyFileSync(source, path.join(stage, name));
  }
  const archive = path.join(dist, archiveName(target));
  if (target.includes('windows')) {
    execFileSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command',
      'Compress-Archive -LiteralPath $env:POLYFW_STAGE -DestinationPath $env:POLYFW_ARCHIVE -Force'],
    { env: { ...process.env, POLYFW_STAGE: stage, POLYFW_ARCHIVE: archive }, stdio: 'inherit' });
  } else {
    execFileSync('tar', ['-czf', archive, '-C', dist, path.basename(stage)], { stdio: 'inherit' });
  }
  fs.rmSync(stage, { recursive: true });
  return archive;
}

export async function checkCrate(version, checksum, fetcher = fetch) {
  const response = await fetcher(`https://crates.io/api/v1/crates/polyfw/${encodeURIComponent(version)}`, {
    headers: { 'User-Agent': 'polyfw-release (https://github.com/inaku-Gyan/polyfw)' },
  });
  if (response.status === 404) return false;
  assert(response.ok, `crates.io returned HTTP ${response.status}`);
  const { version: published } = await response.json();
  assert.equal(published?.num, version, 'Registry returned a different version');
  assert.equal(published?.checksum, checksum, 'Published crate differs from this tagged package; use a new version');
  assert(!published.yanked, 'The existing crate version is yanked; use a new version');
  return true;
}

async function remoteTag(options) {
  const { github, owner, repo, tag, commit } = options;
  const response = await github.rest.repos.getCommit({ owner, repo, ref: tag });
  assert.equal(response.data.sha, commit, 'Remote tag differs from the validated commit');
}

const marker = ({ commit, checksum }) => `<!-- polyfw-release:${commit}:${checksum} -->`;

async function findRelease({ github, owner, repo, tag }) {
  try { return (await github.rest.repos.getReleaseByTag({ owner, repo, tag })).data; }
  catch (error) { if (error.status !== 404) throw error; }
  // The tag endpoint only promises published releases; drafts need the authenticated list.
  const releases = await github.paginate(github.rest.repos.listReleases, { owner, repo, per_page: 100 });
  const matches = releases.filter(release => release.tag_name === tag);
  assert(matches.length <= 1, 'Multiple releases use this tag; resolve the duplicate drafts first');
  return matches[0] ?? null;
}

function verifyIdentity(release, options) {
  assert.equal(release.tag_name, options.tag, 'Release tag differs');
  assert(release.body?.includes(marker(options)), 'Release belongs to different source/package; refusing to replace it');
  assert.equal(release.prerelease, !!parseVersion(options.version).prerelease.length, 'Release channel differs');
}

async function assetBytes(options, asset) {
  const response = await options.github.rest.repos.getReleaseAsset({
    owner: options.owner, repo: options.repo, asset_id: asset.id,
    headers: { accept: 'application/octet-stream' },
  });
  return Buffer.from(response.data);
}

async function assetHash(options, asset) {
  return /^sha256:([0-9a-f]{64})$/.exec(asset.digest ?? '')?.[1] ?? sha256(await assetBytes(options, asset));
}

export async function verifyAssets(options, release) {
  const names = archiveNames();
  assert.deepEqual(release.assets.map(asset => asset.name).sort(), [...names, 'SHA256SUMS'].sort(), 'Release asset set is incomplete');
  const manifest = await assetBytes(options, release.assets.find(asset => asset.name === 'SHA256SUMS'));
  const checksums = new Map(manifest.toString('utf8').trimEnd().split('\n').map(line => {
    const match = /^([0-9a-f]{64})  (polyfw-[a-z0-9_-]+\.(?:tar\.gz|zip))$/.exec(line);
    assert(match, 'Invalid SHA256SUMS entry');
    return [match[2], match[1]];
  }));
  assert.deepEqual([...checksums.keys()].sort(), names, 'SHA256SUMS does not cover every target');
  for (const asset of release.assets) {
    assert.equal(asset.state, 'uploaded', `Incomplete upload: ${asset.name}`);
    if (asset.name !== 'SHA256SUMS') assert.equal(await assetHash(options, asset), checksums.get(asset.name), `Checksum mismatch: ${asset.name}`);
  }
}

export async function prepareRelease(options) {
  const { github, owner, repo, tag, version, commit, dist } = options;
  await remoteTag(options);
  const names = archiveNames();
  assert.deepEqual(fs.readdirSync(dist).filter(name => name !== 'SHA256SUMS').sort(), names, 'Expected exactly one binary archive for every target');
  let release = await findRelease(options);
  if (release) {
    verifyIdentity(release, options);
    if (!release.draft) {
      // Rebuilt archives need not be byte-identical. Published assets are authoritative.
      await verifyAssets(options, release);
      return;
    }
  } else {
    const releases = await github.paginate(github.rest.repos.listReleases, { owner, repo, per_page: 100 });
    const base = notesBase(releases, version);
    const notes = await github.rest.repos.generateReleaseNotes({
      owner, repo, tag_name: tag, target_commitish: commit, ...(base ? { previous_tag_name: base } : {}),
    });
    release = (await github.rest.repos.createRelease({
      owner, repo, tag_name: tag, target_commitish: commit, name: tag,
      draft: true, prerelease: !!parseVersion(version).prerelease.length, make_latest: 'false',
      body: `${notes.data.body}\n\n${marker(options)}`,
    })).data;
  }
  fs.writeFileSync(path.join(dist, 'SHA256SUMS'), names.map(name => `${sha256(fs.readFileSync(path.join(dist, name)))}  ${name}\n`).join(''));
  for (const name of [...names, 'SHA256SUMS']) {
    const data = fs.readFileSync(path.join(dist, name));
    const existing = release.assets.find(asset => asset.name === name);
    if (existing?.state === 'uploaded' && await assetHash(options, existing) === sha256(data)) continue;
    // Drafts can recover partial uploads. Published releases never enter this branch.
    if (existing) await github.rest.repos.deleteReleaseAsset({ owner, repo, asset_id: existing.id });
    await github.rest.repos.uploadReleaseAsset({
      owner, repo, release_id: release.id, name, data,
      headers: { 'content-type': 'application/octet-stream', 'content-length': data.length },
    });
  }
  await verifyAssets(options, await findRelease(options));
}

export async function finishRelease(options) {
  const { github, owner, repo, version } = options;
  await remoteTag(options);
  assert(await checkCrate(version, options.checksum, options.fetcher), 'Crate is not published yet');
  const release = await findRelease(options);
  assert(release, 'GitHub draft is missing');
  verifyIdentity(release, options);
  await verifyAssets(options, release);
  if (!release.draft) return;
  const prerelease = !!parseVersion(version).prerelease.length;
  const releases = await github.paginate(github.rest.repos.listReleases, { owner, repo, per_page: 100 });
  const newerStable = releases.some(entry => {
    try { return !entry.draft && !entry.prerelease && compareVersions(entry.tag_name.slice(1), version) > 0; }
    catch { return false; }
  });
  await github.rest.repos.updateRelease({
    owner, repo, release_id: release.id, draft: false, prerelease,
    make_latest: !prerelease && !newerStable ? 'true' : 'false',
  });
}

// Small interface for actions/github-script; state transitions remain locally testable.
export async function run(stage, { github, context, core, env = process.env }) {
  const root = env.GITHUB_WORKSPACE;
  const options = {
    github, ...context.repo, root, dist: path.join(root, 'dist'),
    tag: env.RELEASE_TAG, version: env.RELEASE_VERSION,
    commit: env.RELEASE_COMMIT, checksum: env.CRATE_CHECKSUM,
  };
  if (stage === 'validate') {
    const outputs = validateTag(root, env.RELEASE_TAG, context.sha);
    for (const [key, value] of Object.entries(outputs)) core.setOutput(key, typeof value === 'object' ? JSON.stringify(value) : value);
  } else if (stage === 'package') {
    packageBinary(root, env.RELEASE_TARGET, env.RELEASE_VERSION);
  } else if (stage === 'crate-checksum') {
    core.setOutput('checksum', sha256(fs.readFileSync(path.join(root, 'target', 'package', `polyfw-${env.RELEASE_VERSION}.crate`))));
  } else if (stage === 'check-crate') {
    await remoteTag(options);
    core.setOutput('exists', await checkCrate(options.version, options.checksum));
  } else if (stage === 'verify-crate') {
    for (let attempt = 0; attempt < 6; attempt++) {
      if (await checkCrate(options.version, options.checksum)) return;
      await new Promise(resolve => setTimeout(resolve, 5000));
    }
    throw new Error('Published crate is not visible yet; rerun this tag after checking crates.io');
  } else if (stage === 'prepare') {
    await prepareRelease(options);
  } else if (stage === 'finish') {
    await finishRelease(options);
  } else {
    throw new Error(`Unknown release stage: ${stage}`);
  }
}
