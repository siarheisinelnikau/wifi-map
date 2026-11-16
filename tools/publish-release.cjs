'use strict';

const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');

const REPOSITORY = 'siarheisinelnikau/wifi-map';
const TAG = 'v0.2.0';
const ASSETS = new Set([
  'WifiMap-v0.2.0.exe', 'WifiMap-v0.2.0.exe.sha256',
  'WifiMap-v0.2.0-debug.apk', 'WifiMap-v0.2.0-debug.apk.sha256',
  'LICENSE', 'WifiMap-Windows-THIRD_PARTY_NOTICES.txt',
  'WifiMap-Android-THIRD_PARTY_NOTICES.txt',
]);
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const blobSha = bytes => crypto.createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
const check = (condition, message) => { if (!condition) throw new Error(message); };
const notFound = async request => { try { return (await request()).data; } catch (error) { if (error.status === 404) return null; throw error; } };

function validateConfig(config) {
  check(config.repository === REPOSITORY && config.tag_name === TAG, 'Unexpected release target');
  check(/^[a-f0-9]{40}$/.test(config.target_commitish), 'A pinned source commit is required');
  check(config.name === 'WifiMap 0.2.0 Preview' && config.prerelease === true && config.draft === false && config.make_latest === 'false', 'This publisher only publishes the approved preview');
  check(typeof config.body === 'string' && config.body.length > 1000 && config.body.includes('Preview limits'), 'Release notes are required');
  check(config.assets.length === ASSETS.size && new Set(config.assets.map(a => a.name)).size === ASSETS.size, 'Unexpected asset list');
  for (const asset of config.assets) {
    check(ASSETS.has(asset.name) && Number.isSafeInteger(asset.size) && asset.size > 0 && asset.size < 16 * 1024 * 1024 && /^[a-f0-9]{64}$/.test(asset.sha256), 'Invalid release asset');
    check(Array.isArray(asset.chunks) && asset.chunks.length > 0 && asset.chunks.length <= 32, 'Missing asset chunks');
    check(asset.chunks.reduce((sum, chunk) => sum + chunk.size, 0) === asset.size, 'Incomplete asset chunk manifest');
    for (const chunk of asset.chunks) check(/^[a-f0-9]{40}$/.test(chunk.sha) && Number.isSafeInteger(chunk.size) && chunk.size > 0 && chunk.size <= 512 * 1024, 'Invalid asset chunk');
  }
}

async function resolveTag(github, repo, tag) {
  let item = await notFound(() => github.rest.git.getRef({ ...repo, ref: `tags/${tag}` }));
  if (!item) return null;
  item = item.object;
  for (let count = 0; count < 4 && item.type === 'tag'; count++) item = (await github.rest.git.getTag({ ...repo, tag_sha: item.sha })).data.object;
  check(item.type === 'commit', 'Release tag must resolve to a commit');
  return item.sha;
}

async function checkSource(github, repo, config, currentSha) {
  for (const [platform, expectedPath] of [['windows', '.github/workflows/windows.yml'], ['android', '.github/workflows/android.yml']]) {
    const runId = config.verified_ci_runs[platform];
    check(Number.isSafeInteger(runId) && runId > 0, 'A verified build run is required');
    const run = (await github.rest.actions.getWorkflowRun({ ...repo, run_id: runId })).data;
    check(run.head_sha === config.target_commitish && run.status === 'completed' && run.conclusion === 'success' && run.path === expectedPath, `The ${platform} build has not passed for the release commit`);
  }
  const trees = [];
  for (const sha of [config.target_commitish, currentSha]) {
    const commit = (await github.rest.git.getCommit({ ...repo, commit_sha: sha })).data;
    trees.push((await github.rest.git.getTree({ ...repo, tree_sha: commit.tree.sha })).data.tree);
  }
  for (const name of ['windows', 'android', 'shared', 'LICENSE']) {
    const before = trees[0].find(entry => entry.path === name);
    const after = trees[1].find(entry => entry.path === name);
    check(before && after && before.sha === after.sha, `Unverified changes in ${name}`);
  }
  const existingTag = await resolveTag(github, repo, config.tag_name);
  check(existingTag === null || existingTag === config.target_commitish, 'An existing tag points to another commit');
}

async function restoreAssets(github, repo, config, output) {
  await fs.mkdir(output, { recursive: true });
  for (const asset of config.assets) {
    const chunks = [];
    for (const chunk of asset.chunks) {
      const blob = (await github.rest.git.getBlob({ ...repo, file_sha: chunk.sha })).data;
      check(blob.encoding === 'base64', 'Unexpected blob encoding');
      const bytes = Buffer.from(blob.content.replace(/\s/g, ''), 'base64');
      check(bytes.length === chunk.size && blob.size === chunk.size && blobSha(bytes) === chunk.sha, `Damaged chunk for ${asset.name}`);
      chunks.push(bytes);
    }
    const bytes = Buffer.concat(chunks);
    check(bytes.length === asset.size && sha256(bytes) === asset.sha256, `Asset checksum mismatch: ${asset.name}`);
    await fs.writeFile(path.join(output, asset.name), bytes);
  }
  for (const name of ['WifiMap-v0.2.0.exe', 'WifiMap-v0.2.0-debug.apk']) {
    const asset = config.assets.find(a => a.name === name);
    const text = await fs.readFile(path.join(output, `${name}.sha256`), 'ascii');
    check(text === `${asset.sha256}  ${name}\n`, `Invalid checksum sidecar: ${name}`);
  }
}

function verifyPackages(root, output) {
  const python = process.platform === 'win32' ? 'python' : 'python3';
  execFileSync(python, [path.join(root, 'windows/tools/verify_artifact.py'), path.join(output, 'WifiMap-v0.2.0.exe')], { cwd: root, stdio: 'inherit', timeout: 60_000 });
  check(process.env.ANDROID_HOME && process.env.JAVA_HOME, 'Android SDK and Java are required for APK verification');
  execFileSync(python, [path.join(root, 'android/tools/verify_apk.py'), path.join(output, 'WifiMap-v0.2.0-debug.apk'), '--sdk', process.env.ANDROID_HOME, '--java-home', process.env.JAVA_HOME], { cwd: root, stdio: 'inherit', timeout: 60_000 });
}

function matchesAsset(remote, expected) {
  return remote.name === expected.name && remote.size === expected.size && remote.state === 'uploaded' && remote.digest === `sha256:${expected.sha256}`;
}

module.exports = async function publish({ github, context, core, root = process.cwd(), config: suppliedConfig, verify = verifyPackages }) {
  check(`${context.repo.owner}/${context.repo.repo}` === REPOSITORY && context.ref === 'refs/heads/main', 'Publishing is restricted to the canonical main branch');
  const repo = context.repo;
  const config = suppliedConfig || JSON.parse(await fs.readFile(path.join(root, '.github/release-v0.2.0.json'), 'utf8'));
  validateConfig(config);
  await checkSource(github, repo, config, context.sha);
  const releases = await github.paginate(github.rest.repos.listReleases, { ...repo, per_page: 100 });
  let release = releases.find(item => item.tag_name === config.tag_name);
  if (release) {
    check(release.name === config.name && release.body === config.body && release.prerelease === true, 'An existing release has different metadata; it will not be changed');
    if (!release.draft) {
      const assets = await github.paginate(github.rest.repos.listReleaseAssets, { ...repo, release_id: release.id, per_page: 100 });
      check(assets.length === config.assets.length && config.assets.every(expected => assets.some(item => matchesAsset(item, expected))), 'The published release differs; it will not be changed');
      core.notice(`The matching preview is already published: ${release.html_url}`);
      return release.html_url;
    }
    check(release.target_commitish === config.target_commitish, 'An existing draft targets another commit');
  }
  const output = path.join(root, 'dist/publish-v0.2.0');
  await restoreAssets(github, repo, config, output);
  await verify(root, output);
  if (!release) release = (await github.rest.repos.createRelease({ ...repo, tag_name: config.tag_name, target_commitish: config.target_commitish, name: config.name, body: config.body, draft: true, prerelease: true, make_latest: 'false', generate_release_notes: false })).data;
  let assets = await github.paginate(github.rest.repos.listReleaseAssets, { ...repo, release_id: release.id, per_page: 100 });
  for (const expected of config.assets) {
    const existing = assets.find(item => item.name === expected.name);
    if (existing) { check(matchesAsset(existing, expected), `An existing draft asset differs: ${expected.name}`); continue; }
    const data = await fs.readFile(path.join(output, expected.name));
    const uploaded = (await github.rest.repos.uploadReleaseAsset({ ...repo, release_id: release.id, name: expected.name, data, headers: { 'content-type': expected.name.endsWith('.exe') || expected.name.endsWith('.apk') ? 'application/octet-stream' : 'text/plain', 'content-length': data.length } })).data;
    check(matchesAsset(uploaded, expected), `Uploaded asset did not verify: ${expected.name}`);
    core.info(`Verified release asset: ${expected.name}`);
  }
  assets = await github.paginate(github.rest.repos.listReleaseAssets, { ...repo, release_id: release.id, per_page: 100 });
  check(assets.length === config.assets.length && config.assets.every(expected => assets.some(item => matchesAsset(item, expected))), 'The draft is incomplete; it remains unpublished');
  release = (await github.rest.repos.updateRelease({ ...repo, release_id: release.id, draft: false, prerelease: true, make_latest: 'false' })).data;
  check(release.draft === false && release.prerelease === true && await resolveTag(github, repo, config.tag_name) === config.target_commitish, 'Published release verification failed');
  core.notice(`Published ${release.html_url}`);
  return release.html_url;
};

module.exports.sha256 = sha256;
module.exports.blobSha = blobSha;
