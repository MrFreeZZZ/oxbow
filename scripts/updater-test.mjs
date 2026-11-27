// Native macOS updater acceptance fixture. Never publishes or uses production signing keys.
// Build isolated .app bundles and serve their signed updates over local HTTPS.
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:https';
import { readFileSync, writeFileSync, mkdirSync, existsSync, cpSync, chmodSync, openSync, closeSync, appendFileSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { macInfoPlist, verifyArtifact } from './release.mjs';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const root = fileURLToPath(new URL('../', import.meta.url));
// Keep the changing ad-hoc-signed app outside macOS's protected Documents folder.
const fixture = '/private/tmp/oxbow-updater-migration';
const cli = join(root, 'app/node_modules/@tauri-apps/cli/tauri.js');
const product = 'Oxbow Updater Test';
const app = `${product}.app`;
const repositories = ['/private/tmp/oxbow-updater-test-repository', '/private/tmp/oxbow-updater-test-second'];
const sha = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim();
const command = process.argv[2];
const builds = {
  '0.1.747': 'preview',
  '0.1.750': 'release',
  '2026.8.1': 'release',
  '2026.9.0-preview.748': 'preview',
  '2026.9.0-preview.749': 'preview',
};
const firstPreview = '2026.9.0-preview.748';
const firstRelease = '2026.8.1';
const modes = ['valid', 'bad-signature', 'bad-bytes', 'wrong-version', 'wrong-channel', 'missing-target', 'older', 'offline', 'truncated', 'slow', 'no-length'];
mkdirSync(fixture, { recursive: true, mode: 0o700 });
function run(exe, args, options = {}) {
  const result = spawnSync(exe, args, { cwd: root, encoding: 'utf8', ...options });
  if (result.status !== 0) throw new Error(`${exe} failed (${result.status}); inspect fixture build logs`);
  return result.stdout;
}
function save(name, value) { writeFileSync(join(fixture, name), value, { mode: 0o600 }); }
const executable = join(app, 'Contents/MacOS/oxbow');
const hash = data => createHash('sha256').update(data).digest('hex');

function snapshot() {
  const settings = JSON.parse(readFileSync(join(process.env.HOME, 'Library/Application Support/app.oxbow.updater-test/settings.json')));
  const selectedChannel = settings['oxbow.updates.channel'] ?? 'auto';
  delete settings['oxbow.updates.channel'];
  return {
    version: JSON.parse(run(join(fixture, executable), ['--build-info'])).version,
    selectedChannel,
    settings: hash(JSON.stringify(Object.fromEntries(Object.entries(settings).sort(([a], [b]) => a.localeCompare(b))))),
    repositories: repositories.map(repo => ({
      path: repo,
      file: hash(readFileSync(join(repo, 'kept.txt'))),
      index: run('git', ['ls-files', '--stage'], { cwd: repo }).trim(),
      head: run('git', ['rev-parse', 'HEAD'], { cwd: repo }).trim(),
      status: run('git', ['--no-optional-locks', 'status', '--porcelain'], { cwd: repo }).trim(),
    })),
  };
}

if (command === 'snapshot') {
  save('before.json', JSON.stringify(snapshot(), null, 2));
  console.log('Saved data snapshot before installation');
} else if (command === 'verify') {
  const before = JSON.parse(readFileSync(join(fixture, 'before.json')));
  const after = snapshot();
  const [version, selectedChannel] = process.argv.slice(3);
  assert(Object.hasOwn(builds, version), 'Specify the expected installed version');
  assert.equal(after.version, version);
  assert.notEqual(before.version, after.version, 'Version did not change through installation');
  assert.equal(after.selectedChannel, selectedChannel);
  assert.equal(after.settings, before.settings, 'Settings changed through installation');
  assert.deepEqual(after.repositories, before.repositories, 'Repository data changed through installation');
  const archive = join(fixture, version, `${app}.tar.gz`);
  verifyArtifact(readFileSync(archive), readFileSync(`${archive}.sig`, 'utf8').trim(), readFileSync(join(fixture, 'key.pub'), 'utf8').trim(), version);
  const signedExecutable = run('tar', ['-xOf', archive, executable], { encoding: null, maxBuffer: 128 * 1024 * 1024 });
  const binaryHash = hash(readFileSync(join(fixture, executable)));
  assert.equal(binaryHash, hash(signedExecutable));
  appendFileSync(join(fixture, 'verified.jsonl'), JSON.stringify({ at: new Date().toISOString(), before, after, binaryHash }) + '\n');
  console.log(`Verified ${before.version} → ${version}: signed executable, selected channel ${selectedChannel}, and unchanged settings and repository data`);
} else if (command === 'build') {
  if (process.platform !== 'darwin') throw new Error('This native fixture currently supports macOS only');
  if (!existsSync(join(fixture, 'key'))) {
    run(process.execPath, [cli, 'signer', 'generate', '--ci', '--password', '', '--write-keys', join(fixture, 'key')], { stdio: 'ignore' });
    chmodSync(join(fixture, 'key'), 0o600);
  }
  const certificate = join(fixture, 'certificate.pem');
  if (!existsSync(certificate) || spawnSync('openssl', ['x509', '-checkend', '3600', '-noout', '-in', certificate], { stdio: 'ignore' }).status !== 0) {
    save('tls.cnf', '[req]\ndistinguished_name=dn\nx509_extensions=v3\nprompt=no\n[dn]\nCN=Oxbow updater localhost fixture\n[v3]\nsubjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,digitalSignature,keyEncipherment,keyCertSign\nextendedKeyUsage=serverAuth\n');
    run('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '3', '-config', join(fixture, 'tls.cnf'), '-keyout', join(fixture, 'tls.key'), '-out', join(fixture, 'certificate.pem')], { stdio: 'ignore' });
    chmodSync(join(fixture, 'tls.key'), 0o600);
  }
  for (const [version, channel] of Object.entries(builds)) {
    const config = { productName: product, identifier: 'app.oxbow.updater-test', version, bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey: readFileSync(join(fixture, 'key.pub'), 'utf8').trim(), requireSignedVersion: true } } };
    const plist = join(fixture, `Info-${version}.plist`);
    save(`Info-${version}.plist`, macInfoPlist(version));
    config.bundle.macOS = { bundleVersion: version.split('-')[0], infoPlist: plist };
    save(`config-${version}.json`, JSON.stringify(config));
    const env = { ...process.env, APPLE_SIGNING_IDENTITY: '-', OXBOW_UPDATER_TEST_ROOT: fixture, OXBOW_BUILD_CHANNEL: channel, OXBOW_BUILD_VERSION: version, OXBOW_BUILD_COMMIT: sha, OXBOW_BASE_RELEASE: '2026.8.0', OXBOW_COMMITS_SINCE_RELEASE: '52', TAURI_SIGNING_PRIVATE_KEY: readFileSync(join(fixture, 'key'), 'utf8'), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' };
    const log = openSync(join(fixture, `build-${version}.log`), 'w', 0o600);
    try {
      run(process.execPath, [cli, 'build', '--ci', '--features', 'updater-test', '--bundles', 'app', '--config', JSON.stringify(config)], { cwd: join(root, 'app'), env, stdio: ['ignore', log, log] });
    } finally { closeSync(log); }
    const dest = join(fixture, version);
    mkdirSync(dest, { recursive: true });
    for (const name of [app, `${app}.tar.gz`, `${app}.tar.gz.sig`]) {
      rmSync(join(dest, name), { recursive: true, force: true });
      cpSync(join(root, 'target/release/bundle/macos', name), join(dest, name), { recursive: true });
    }
    verifyArtifact(readFileSync(join(dest, `${app}.tar.gz`)), readFileSync(join(dest, `${app}.tar.gz.sig`), 'utf8').trim(), readFileSync(join(fixture, 'key.pub'), 'utf8').trim(), version);
    const metadata = JSON.parse(run(join(dest, app, 'Contents/MacOS/oxbow'), ['--build-info']));
    assert.equal(metadata.version, version);
    assert.equal(metadata.buildChannel, channel);
    assert.equal(metadata.baseRelease, '2026.8.0');
    console.log(`Built and verified isolated signed ${version}`);
  }
  // Do not leave test packages beside the production artifacts collected by release.mjs.
  for (const name of [app, `${app}.tar.gz`, `${app}.tar.gz.sig`]) rmSync(join(root, 'target/release/bundle/macos', name), { recursive: true, force: true });
  rmSync(join(fixture, 'restart-failed'), { force: true });
  save('mode', 'valid');
  save('preview-version', firstPreview);
  for (const name of ['states.jsonl', 'requests.jsonl', 'launches.jsonl', 'verified.jsonl']) save(name, '');
  console.log('Packages ready. Close the fixture app before running start.');
} else if (command === 'start') {
  const version = process.argv[3];
  assert(['0.1.747', '0.1.750'].includes(version), 'Choose the legacy Preview or Release starting version');
  rmSync(join(fixture, app), { recursive: true, force: true });
  cpSync(join(fixture, version, app), join(fixture, app), { recursive: true });
  console.log(join(fixture, app));
} else if (command === 'advance-preview') {
  save('preview-version', '2026.9.0-preview.749');
} else if (command === 'mode') {
  if (!modes.includes(process.argv[3])) throw new Error(`Choose: ${modes.join(', ')}`);
  save('mode', process.argv[3]);
  console.log(`Fixture mode: ${process.argv[3]}`);
} else if (command === 'serve') {
  const packages = new Map([firstPreview, '2026.9.0-preview.749', firstRelease].map(version => [
    version, {
      bytes: readFileSync(join(fixture, version, `${app}.tar.gz`)),
      signature: readFileSync(join(fixture, version, `${app}.tar.gz.sig`), 'utf8').trim(),
    },
  ]));
  const server = createServer({ key: readFileSync(join(fixture, 'tls.key')), cert: readFileSync(join(fixture, 'certificate.pem')) }, (req, res) => {
    const mode = readFileSync(join(fixture, 'mode'), 'utf8').trim();
    appendFileSync(join(fixture, 'requests.jsonl'), JSON.stringify({ at: Date.now(), path: req.url, mode }) + '\n');
    if (mode === 'offline') { res.writeHead(503); res.end(); return; }
    if (/^\/(preview|release)\/latest.json$/.test(req.url)) {
      const channel = req.url.split('/')[1];
      const target = channel === 'preview' ? readFileSync(join(fixture, 'preview-version'), 'utf8').trim() : firstRelease;
      const version = mode === 'older' ? '0.0.0' : mode === 'wrong-version' ? '2099.1.0' : target;
      let sig = packages.get(target).signature;
      if (mode === 'bad-signature') {
        const lines = Buffer.from(sig, 'base64').toString().split('\n');
        const bytes = Buffer.from(lines[1], 'base64'); bytes[bytes.length - 1] ^= 1; lines[1] = bytes.toString('base64');
        sig = Buffer.from(lines.join('\n')).toString('base64');
      }
      const tag = channel === 'preview' ? `preview-${sha}` : `v${version}`;
      const arch = process.arch === 'arm64' ? 'aarch64' : 'x86_64';
      const otherArch = arch === 'aarch64' ? 'x86_64' : 'aarch64';
      const notes = `## Updater acceptance\n\nSigned macOS updater fixture for ${version}.\n\n`
        + Array.from({ length: 80 }, (_, i) => `- Release note ${i + 1}: long notes stay inside their scroll area.`).join('\n');
      const manifest = { version, channel: mode === 'wrong-channel' ? 'foreign' : channel, commit: sha, notes, platforms: { [`darwin-${mode === 'missing-target' ? otherArch : arch}`]: { url: `https://github.com/MrFreeZZZ/oxbow/releases/download/${tag}/Oxbow-test-${target}.app.tar.gz`, signature: sig } } };
      res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); res.end(JSON.stringify(manifest)); return;
    }
    const version = /^\/MrFreeZZZ\/oxbow\/releases\/download\/[^/]+\/Oxbow-test-(.+)\.app\.tar\.gz$/.exec(req.url)?.[1];
    const artifact = packages.get(version);
    if (!artifact) { res.writeHead(404); res.end(); return; }
    const data = Buffer.from(artifact.bytes);
    if (mode === 'bad-bytes') data[data.length - 1] ^= 1;
    res.writeHead(200, { 'Content-Type': 'application/octet-stream', ...(mode === 'no-length' ? {} : { 'Content-Length': data.length }), 'Cache-Control': 'no-store' });
    if (mode === 'truncated') { res.write(data.subarray(0, 4096)); setTimeout(() => res.destroy(), 200); return; }
    let offset = 0;
    const timer = setInterval(() => {
      const next = Math.min(offset + 64 * 1024, data.length);
      res.write(data.subarray(offset, next)); offset = next;
      if (offset === data.length) { clearInterval(timer); res.end(); }
    }, mode === 'slow' ? 180 : 2);
    res.on('close', () => clearInterval(timer));
  });
  server.listen(43179, '127.0.0.1', () => console.log('Updater fixture: https://127.0.0.1:43179 (loopback only)'));
} else {
  throw new Error('Usage: node scripts/updater-test.mjs build | start <legacy-version> | serve | advance-preview | mode <scenario> | snapshot | verify <version> <auto|release|preview>');
}
