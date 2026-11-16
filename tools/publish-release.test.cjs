'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const publish = require('./publish-release.cjs');
const repository = path.resolve(__dirname, '..');

async function fixture(options = {}) {
  await fs.mkdir(path.join(repository, 'dist'), { recursive: true });
  const root = await fs.mkdtemp(path.join(repository, 'dist/publisher-tests-'));
  const config = {
    repository: 'siarheisinelnikau/wifi-map', tag_name: 'v0.2.0', name: 'WifiMap 0.2.0 Preview',
    target_commitish: 'a'.repeat(40), prerelease: true, draft: false, make_latest: 'false',
    body: 'Preview limits\n' + 'Synthetic publisher fixture. '.repeat(50),
    verified_ci_runs: { windows: 1, android: 2 }, assets: [],
  };
  const blobs = new Map();
  const add = (name, bytes) => {
    const sha = publish.blobSha(bytes);
    blobs.set(sha, bytes);
    config.assets.push({ name, size: bytes.length, sha256: publish.sha256(bytes), chunks: [{ sha, size: bytes.length }] });
  };
  for (const name of ['WifiMap-v0.2.0.exe', 'WifiMap-v0.2.0-debug.apk']) {
    add(name, Buffer.from(`Synthetic bytes for ${name}`));
    add(`${name}.sha256`, Buffer.from(`${config.assets.at(-1).sha256}  ${name}\n`));
  }
  for (const name of ['LICENSE', 'WifiMap-Windows-THIRD_PARTY_NOTICES.txt', 'WifiMap-Android-THIRD_PARTY_NOTICES.txt']) add(name, Buffer.from(`Synthetic notice fixture: ${name}`));
  const state = { creates: 0, uploads: 0, publishes: 0, assets: [], release: null, tag: null };
  const listReleases = () => {};
  const listReleaseAssets = () => {};
  const github = {
    rest: {
      actions: { getWorkflowRun: async ({ run_id }) => ({ data: { head_sha: config.target_commitish, status: 'completed', conclusion: 'success', path: `.github/workflows/${run_id === 1 ? 'windows' : 'android'}.yml` } }) },
      git: {
        getCommit: async () => ({ data: { tree: { sha: 'c'.repeat(40) } } }),
        getTree: async () => ({ data: { tree: ['windows', 'android', 'shared', 'LICENSE'].map(name => ({ path: name, sha: 'd'.repeat(40) })) } }),
        getRef: async () => { if (!state.tag) throw Object.assign(new Error('Not found'), { status: 404 }); return { data: { object: { type: 'commit', sha: state.tag } } }; },
        getBlob: async ({ file_sha }) => ({ data: { encoding: 'base64', content: blobs.get(file_sha).toString('base64'), size: blobs.get(file_sha).length } }),
      },
      repos: {
        listReleases, listReleaseAssets,
        createRelease: async parameters => { assert.equal(parameters.draft, true); state.creates++; state.release = { ...parameters, id: 9, html_url: 'https://github.com/siarheisinelnikau/wifi-map/releases/tag/v0.2.0' }; return { data: state.release }; },
        uploadReleaseAsset: async ({ name, data }) => { state.uploads++; const asset = { name, size: data.length, state: 'uploaded', digest: `sha256:${publish.sha256(data)}` }; if (options.badUpload && state.uploads === 2) asset.digest = `sha256:${'0'.repeat(64)}`; state.assets.push(asset); return { data: asset }; },
        updateRelease: async parameters => { assert.equal(state.assets.length, config.assets.length); state.publishes++; state.tag = config.target_commitish; state.release = { ...state.release, ...parameters }; return { data: state.release }; },
      },
    },
    paginate: async method => method === listReleases ? (state.release ? [state.release] : []) : state.assets,
  };
  const args = { github, context: { repo: { owner: 'siarheisinelnikau', repo: 'wifi-map' }, ref: 'refs/heads/main', sha: 'b'.repeat(40) }, core: { info() {}, notice() {} }, root, config, verify: async (_, output) => { for (const asset of config.assets) assert.equal(publish.sha256(await fs.readFile(path.join(output, asset.name))), asset.sha256); } };
  const cleanup = async () => { assert.equal(path.dirname(root), path.join(repository, 'dist')); assert.ok(path.basename(root).startsWith('publisher-tests-')); await fs.rm(root, { recursive: true, force: true }); };
  return { args, config, state, cleanup };
}

test('publishes only a complete verified draft and reruns without changing published assets', async () => {
  const f = await fixture();
  try {
    await publish(f.args);
    assert.deepEqual([f.state.creates, f.state.uploads, f.state.publishes], [1, 7, 1]);
    assert.equal(f.state.release.draft, false);
    await publish(f.args);
    assert.deepEqual([f.state.creates, f.state.uploads, f.state.publishes], [1, 7, 1]);
  } finally { await f.cleanup(); }
});

test('rejects damaged input before creating a release', async () => {
  const f = await fixture();
  try {
    f.config.assets[0].sha256 = '0'.repeat(64);
    await assert.rejects(publish(f.args), /Asset checksum mismatch/);
    assert.equal(f.state.creates, 0);
  } finally { await f.cleanup(); }
});

test('leaves a draft unpublished when an uploaded asset does not verify', async () => {
  const f = await fixture({ badUpload: true });
  try {
    await assert.rejects(publish(f.args), /Uploaded asset did not verify/);
    assert.equal(f.state.release.draft, true);
    assert.equal(f.state.publishes, 0);
  } finally { await f.cleanup(); }
});

test('refuses to edit a published release with different metadata', async () => {
  const f = await fixture();
  try {
    f.state.release = { tag_name: 'v0.2.0', name: 'Existing release', draft: false };
    await assert.rejects(publish(f.args), /will not be changed/);
    assert.deepEqual([f.state.creates, f.state.uploads, f.state.publishes], [0, 0, 0]);
  } finally { await f.cleanup(); }
});
