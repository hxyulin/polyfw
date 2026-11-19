import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  archiveName, checkCrate, compareVersions, finishRelease, notesBase,
  packageBinary, parseVersion, prepareRelease, sha256, targets, validateTag,
} from './release.mjs';

const release = (version, extra = {}) => ({ tag_name: `v${version}`, draft: false, prerelease: !!parseVersion(version).prerelease.length, ...extra });

test('SemVer ordering and channel detection cover prereleases and hyphenated build metadata', () => {
  const versions = ['1.0.0-alpha', '1.0.0-alpha.1', '1.0.0-alpha.beta', '1.0.0-beta', '1.0.0-beta.2', '1.0.0-beta.11', '1.0.0-rc.1', '1.0.0'];
  for (let i = 1; i < versions.length; i++) assert(compareVersions(versions[i - 1], versions[i]) < 0);
  assert(compareVersions('0.2.0', '0.10.0') < 0);
  assert(compareVersions('1.0.0-9007199254740992', '1.0.0-9007199254740993') < 0);
  assert.equal(compareVersions('1.0.0+build-one', '1.0.0+build-two'), 0);
  assert.deepEqual(parseVersion('1.0.0+build-with-hyphens').prerelease, []);
  for (const invalid of ['1.2', 'v1.2.3', '01.2.3', '1.2.3-01', '1.2.3-', '1.2.3+']) {
    assert.throws(() => parseVersion(invalid));
  }
});

test('release notes compare stable releases separately from the current prerelease series', () => {
  const releases = [
    release('0.1.0'), release('0.2.0-beta.1'), release('0.2.0-rc.2'), release('0.2.0-rc.10'),
    release('0.3.0-rc.1'), release('0.4.0', { draft: true }),
  ];
  assert.equal(notesBase(releases, '0.2.0'), 'v0.1.0');
  assert.equal(notesBase(releases, '0.2.0-rc.11'), 'v0.2.0-rc.10');
  assert.equal(notesBase(releases, '0.2.0-alpha.1'), 'v0.1.0');
  assert.equal(notesBase(releases, '0.3.0-beta.1'), 'v0.1.0');
  assert.equal(notesBase([], '0.1.0-rc.1'), undefined);
});

test('tag validation rejects version mismatch, commits outside main, and moved tags', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'polyfw-release-git-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  fs.mkdirSync(path.join(root, 'src'));
  fs.writeFileSync(path.join(root, 'Cargo.toml'), '[package]\nname = "polyfw"\nversion = "0.2.0-rc.1"\nedition = "2024"\n');
  fs.writeFileSync(path.join(root, 'src/main.rs'), 'fn main() {}\n');
  fs.copyFileSync(fileURLToPath(new URL('../../rust-toolchain.toml', import.meta.url)), path.join(root, 'rust-toolchain.toml'));
  execFileSync('cargo', ['generate-lockfile', '--offline'], { cwd: root });
  git('init', '--initial-branch=main');
  git('add', '.');
  git('-c', 'user.name=Release Test', '-c', 'user.email=release@example.invalid', 'commit', '-m', 'fixture');
  const commit = git('rev-parse', 'HEAD');
  git('update-ref', 'refs/remotes/origin/main', commit);
  git('-c', 'user.name=Release Test', '-c', 'user.email=release@example.invalid', 'tag', '-a', 'v0.2.0-rc.1', '-m', 'candidate');
  const valid = validateTag(root, 'v0.2.0-rc.1', commit);
  assert.equal(valid.version, '0.2.0-rc.1');
  assert(valid.prerelease);
  assert.equal(valid.rust_toolchain, '1.100.0');
  assert.equal(validateTag(root, 'v0.2.0-rc.1', git('rev-parse', 'refs/tags/v0.2.0-rc.1')).commit, commit);
  if (process.platform !== 'win32') {
    const alias = `${root}-alias`;
    fs.symlinkSync(root, alias);
    t.after(() => fs.unlinkSync(alias));
    assert.equal(validateTag(alias, 'v0.2.0-rc.1', commit).commit, commit);
  }
  git('tag', 'v0.2.0');
  assert.throws(() => validateTag(root, 'v0.2.0', commit), /versions differ/);
  assert.throws(() => validateTag(root, 'v0.2.0-rc.1', 'b'.repeat(40)), /triggering commit/);
  git('checkout', '-b', 'candidate');
  fs.appendFileSync(path.join(root, 'src/main.rs'), '// candidate\n');
  git('add', '.');
  git('-c', 'user.name=Release Test', '-c', 'user.email=release@example.invalid', 'commit', '-m', 'unmerged change');
  const unmerged = git('rev-parse', 'HEAD');
  git('tag', '-f', 'v0.2.0-rc.1');
  assert.throws(() => validateTag(root, 'v0.2.0-rc.1', unmerged), /merged into main/);
  git('tag', '-f', 'v0.2.0-rc.1', commit);
  assert.throws(() => validateTag(root, 'v0.2.0-rc.1', unmerged), /Tag has moved/);
});

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'polyfw-release-state-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const dist = path.join(root, 'dist');
  fs.mkdirSync(dist);
  for (const { target } of targets) fs.writeFileSync(path.join(dist, archiveName(target)), `binary for ${target}`);
  const state = { release: null, history: [], operations: [], nextId: 1, failUpload: null, sha: 'a'.repeat(40) };
  const repos = {
    getCommit: async () => ({ data: { sha: state.sha } }),
    getReleaseByTag: async () => {
      if (!state.release || state.release.draft) throw Object.assign(new Error('Not found'), { status: 404 });
      return { data: structuredClone(state.release) };
    },
    listReleases: async () => ({ data: state.history }),
    generateReleaseNotes: async request => {
      state.operations.push(['notes', request.previous_tag_name]);
      return { data: { body: 'Generated notes' } };
    },
    createRelease: async request => {
      state.operations.push(['create', request.draft]);
      state.release = { ...request, id: 1, assets: [] };
      return { data: structuredClone(state.release) };
    },
    getReleaseAsset: async ({ asset_id }) => ({ data: state.release.assets.find(asset => asset.id === asset_id).bytes }),
    uploadReleaseAsset: async ({ name, data }) => {
      if (name === state.failUpload) {
        state.failUpload = null;
        throw Object.assign(new Error('Interrupted upload'), { status: 502 });
      }
      state.operations.push(['upload', name]);
      state.release.assets.push({ id: state.nextId++, name, bytes: Buffer.from(data), digest: `sha256:${sha256(data)}`, state: 'uploaded' });
      return { data: {} };
    },
    deleteReleaseAsset: async ({ asset_id }) => {
      state.operations.push(['delete', asset_id]);
      state.release.assets = state.release.assets.filter(asset => asset.id !== asset_id);
    },
    updateRelease: async request => {
      state.operations.push(['publish', request.make_latest]);
      Object.assign(state.release, request);
      return { data: structuredClone(state.release) };
    },
  };
  const options = {
    github: { rest: { repos }, paginate: async () => [...state.history, ...(state.release ? [structuredClone(state.release)] : [])] },
    owner: 'example', repo: 'polyfw', tag: 'v0.2.0', version: '0.2.0',
    commit: state.sha, checksum: 'b'.repeat(64), root, dist,
  };
  options.fetcher = async () => ({ ok: true, status: 200, json: async () => ({ version: { num: options.version, checksum: options.checksum, yanked: false } }) });
  return { root, dist, state, options };
}

test('a complete draft is published only after the registry has the identical crate', async t => {
  const { state, options } = fixture(t);
  state.history = [release('0.1.0'), release('0.2.0-rc.1')];
  await prepareRelease(options);
  assert(state.release.draft);
  assert.equal(state.release.assets.length, 6);
  assert.deepEqual(state.operations[0], ['notes', 'v0.1.0']);
  await assert.rejects(finishRelease({ ...options, fetcher: async () => ({ status: 404 }) }), /not published/);
  assert(state.release.draft);
  await finishRelease(options);
  assert(!state.release.draft);
  assert.equal(state.release.make_latest, 'true');
});

test('interrupted draft uploads resume without duplicating successful assets', async t => {
  const { state, options } = fixture(t);
  state.failUpload = archiveName(targets[2].target);
  await assert.rejects(prepareRelease(options), /Interrupted upload/);
  const uploaded = state.release.assets.map(asset => asset.id);
  assert(uploaded.length > 0 && uploaded.length < 6);
  await prepareRelease(options);
  for (const id of uploaded) assert(state.release.assets.some(asset => asset.id === id));
  assert.equal(state.operations.filter(([op]) => op === 'create').length, 1);
  await finishRelease(options);
  assert(!state.release.draft);
});

test('incomplete or changed draft assets can be replaced before publication', async t => {
  const { state, options, dist } = fixture(t);
  await prepareRelease(options);
  const asset = state.release.assets[0];
  asset.state = 'starter';
  fs.writeFileSync(path.join(dist, archiveName(targets[0].target)), 'rebuilt binary');
  await prepareRelease(options);
  assert(state.operations.some(([op]) => op === 'delete'));
  await finishRelease(options);
});

test('re-running after both channels succeeded preserves published bytes', async t => {
  const { state, options, dist } = fixture(t);
  await prepareRelease(options);
  await finishRelease(options);
  const original = structuredClone(state.release);
  const operationCount = state.operations.length;
  for (const { target } of targets) fs.writeFileSync(path.join(dist, archiveName(target)), 'nonidentical rebuild');
  await prepareRelease(options);
  await finishRelease(options);
  assert.deepEqual(structuredClone(state.release), original);
  assert.equal(state.operations.length, operationCount);
});

test('published asset corruption and conflicting source identity stop a rerun', async t => {
  const { state, options } = fixture(t);
  await prepareRelease(options);
  await finishRelease(options);
  state.release.assets[0].digest = `sha256:${'0'.repeat(64)}`;
  await assert.rejects(prepareRelease(options), /Checksum mismatch/);
  state.release.body = 'A release made for different source';
  await assert.rejects(prepareRelease(options), /different source/);
});

test('a remote tag moved after validation stops all publication', async t => {
  const { state, options } = fixture(t);
  state.sha = 'c'.repeat(40);
  await assert.rejects(prepareRelease(options), /Remote tag differs/);
  assert.equal(state.operations.length, 0);
});

test('prereleases never become latest and older stable releases preserve a newer latest', async t => {
  const { state, options } = fixture(t);
  options.version = '0.2.0-rc.1';
  options.tag = `v${options.version}`;
  await prepareRelease(options);
  await finishRelease(options);
  assert(state.release.prerelease);
  assert.equal(state.release.make_latest, 'false');
  state.release = null;
  state.history = [release('0.3.0')];
  options.version = '0.2.0';
  options.tag = 'v0.2.0';
  await prepareRelease(options);
  await finishRelease(options);
  assert.equal(state.release.make_latest, 'false');
});

test('registry bootstrap, transient errors, conflicting versions and yanked versions are distinct', async () => {
  const checksum = 'b'.repeat(64);
  assert.equal(await checkCrate('0.1.0', checksum, async () => ({ status: 404 })), false);
  await assert.rejects(checkCrate('0.1.0', checksum, async () => ({ status: 503, ok: false })), /HTTP 503/);
  const response = version => async () => ({ ok: true, json: async () => ({ version }) });
  assert(await checkCrate('0.1.0', checksum, response({ num: '0.1.0', checksum })));
  await assert.rejects(checkCrate('0.1.0', checksum, response({ num: '0.1.0', checksum: 'c'.repeat(64) })), /differs/);
  await assert.rejects(checkCrate('0.1.0', checksum, response({ num: '0.1.0', checksum, yanked: true })), /yanked/);
});

test('archive layout matches binstall and contains a runnable binary, README and license', t => {
  if (process.platform === 'win32') return t.skip('Unix archive integration test');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'polyfw-release-package-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const target = process.platform === 'darwin' ? 'aarch64-apple-darwin' : 'x86_64-unknown-linux-gnu';
  const binary = path.join(root, 'target', target, 'release', 'polyfw');
  fs.mkdirSync(path.dirname(binary), { recursive: true });
  fs.writeFileSync(binary, '#!/bin/sh\ncase "$1" in\n--version) echo "polyfw 0.2.0-rc.1";;\n--help) echo "Usage: polyfw";;\nschema) echo "{}";;\nesac\n', { mode: 0o755 });
  fs.writeFileSync(path.join(root, 'README.md'), 'Readme');
  fs.writeFileSync(path.join(root, 'LICENSE'), 'MIT');
  const archive = packageBinary(root, target, '0.2.0-rc.1');
  const entries = execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' }).trim().split('\n');
  assert(entries.includes(`polyfw-${target}/polyfw`));
  assert(entries.includes(`polyfw-${target}/README.md`));
  assert(entries.includes(`polyfw-${target}/LICENSE`));
  assert.throws(() => packageBinary(root, target, '0.2.0'), /Binary version differs/);
});
