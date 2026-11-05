// Native macOS updater acceptance fixture. Never publishes or uses production signing keys.
// build: two isolated .app bundles; serve: local HTTPS; mode <name>: inject the next failure.
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:https';
import { readFileSync, writeFileSync, mkdirSync, existsSync, cpSync, chmodSync, openSync, closeSync, appendFileSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const root = fileURLToPath(new URL('../', import.meta.url));
// Keep the changing ad-hoc-signed app outside macOS's protected Documents folder.
const fixture = '/private/tmp/oxbow-updater-acceptance';
const cli = join(root, 'app/node_modules/@tauri-apps/cli/tauri.js');
const product = 'Oxbow Updater Test';
const app = `${product}.app`;
const repository = '/private/tmp/oxbow-updater-test-repository';
const sha = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim();
const command = process.argv[2];
const modes = ['valid', 'bad-signature', 'bad-bytes', 'wrong-version', 'wrong-channel', 'missing-target', 'older', 'offline', 'truncated', 'slow', 'no-length'];
mkdirSync(fixture, { recursive: true, mode: 0o700 });
function run(exe, args, options = {}) {
  const result = spawnSync(exe, args, { cwd: root, encoding: 'utf8', ...options });
  if (result.status !== 0) throw new Error(`${exe} failed (${result.status}); inspect fixture build logs`);
  return result.stdout;
}
function save(name, value) { writeFileSync(join(fixture, name), value, { mode: 0o600 }); }
function snapshot() {
  const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
  const repo = repository;
  return {
    version: JSON.parse(run(join(fixture, app, 'Contents/MacOS/oxbow'), ['--build-info'])).version,
    settings: hash(join(process.env.HOME, 'Library/Application Support/app.oxbow.updater-test/settings.json')),
    file: hash(join(repo, 'kept.txt')),
    index: hash(join(repo, '.git/index')),
    head: run('git', ['rev-parse', 'HEAD'], { cwd: repo }).trim(),
    status: run('git', ['status', '--porcelain'], { cwd: repo }).trim(),
  };
}

if (command === 'snapshot') {
  save('before.json', JSON.stringify(snapshot(), null, 2));
  console.log('Saved data snapshot before installation');
} else if (command === 'verify') {
  const before = JSON.parse(readFileSync(join(fixture, 'before.json')));
  const after = snapshot();
  assert.equal(before.version, '0.0.1');
  assert.equal(after.version, '0.0.2');
  for (const key of ['settings', 'file', 'index', 'head', 'status']) assert.equal(after[key], before[key], `${key} changed through installation`);
  const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
  assert.equal(hash(join(fixture, app, 'Contents/MacOS/oxbow')), hash(join(fixture, '0.0.2', app, 'Contents/MacOS/oxbow')));
  const states = readFileSync(join(fixture, 'states.jsonl'), 'utf8').trim().split('\n').map(line => JSON.parse(line));
  for (const state of ['available', 'downloading', 'verifying', 'ready', 'preparing', 'installing', 'downloadFailed', 'installFailed', 'current']) assert(states.some(item => item.state === state), `Missing ${state}`);
  for (const message of [/signature could not be verified/, /could not be downloaded/, /Save or discard/, /Finish the running Git/]) assert(states.some(item => message.test(item.message ?? '')), `Missing failure ${message}`);
  save('verified.json', JSON.stringify({ at: new Date().toISOString(), before, after, states: [...new Set(states.map(item => item.state))] }, null, 2));
  console.log('Verified signed 0.0.1 → 0.0.2: exact installed binary, unchanged settings/working file/index/HEAD/status and expected lifecycle/failures');
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
  for (const version of ['0.0.2', '0.0.1']) {
    const config = { productName: product, identifier: 'app.oxbow.updater-test', version, bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey: readFileSync(join(fixture, 'key.pub'), 'utf8').trim(), requireSignedVersion: true } } };
    save(`config-${version}.json`, JSON.stringify(config));
    const env = { ...process.env, APPLE_SIGNING_IDENTITY: '-', OXBOW_UPDATER_TEST_ROOT: fixture, OXBOW_BUILD_CHANNEL: 'preview', OXBOW_BUILD_VERSION: version, OXBOW_BUILD_COMMIT: sha, TAURI_SIGNING_PRIVATE_KEY: readFileSync(join(fixture, 'key'), 'utf8'), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' };
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
    console.log(`Built isolated ${version}`);
  }
  rmSync(join(fixture, app), { recursive: true, force: true });
  cpSync(join(fixture, '0.0.1', app), join(fixture, app), { recursive: true });
  // Do not leave test packages beside the production artifacts collected by release.mjs.
  for (const name of [app, `${app}.tar.gz`, `${app}.tar.gz.sig`]) rmSync(join(root, 'target/release/bundle/macos', name), { recursive: true, force: true });
  rmSync(join(fixture, 'restart-failed'), { force: true });
  save('mode', 'valid');
  for (const name of ['states.jsonl', 'requests.jsonl', 'launches.jsonl']) save(name, '');
  console.log(`Ready: ${join(fixture, app)}`);
} else if (command === 'mode') {
  if (!modes.includes(process.argv[3])) throw new Error(`Choose: ${modes.join(', ')}`);
  save('mode', process.argv[3]);
  console.log(`Fixture mode: ${process.argv[3]}`);
} else if (command === 'serve') {
  const packageBytes = readFileSync(join(fixture, '0.0.2', `${app}.tar.gz`));
  const signature = readFileSync(join(fixture, '0.0.2', `${app}.tar.gz.sig`), 'utf8').trim();
  const server = createServer({ key: readFileSync(join(fixture, 'tls.key')), cert: readFileSync(join(fixture, 'certificate.pem')) }, (req, res) => {
    const mode = readFileSync(join(fixture, 'mode'), 'utf8').trim();
    appendFileSync(join(fixture, 'requests.jsonl'), JSON.stringify({ at: Date.now(), path: req.url, mode }) + '\n');
    if (mode === 'offline') { res.writeHead(503); res.end(); return; }
    if (/^\/(preview|release)\/latest.json$/.test(req.url)) {
      const channel = req.url.split('/')[1];
      const version = mode === 'older' ? '0.0.0' : mode === 'wrong-version' ? '0.0.3' : '0.0.2';
      let sig = signature;
      if (mode === 'bad-signature') {
        const lines = Buffer.from(sig, 'base64').toString().split('\n');
        const bytes = Buffer.from(lines[1], 'base64'); bytes[bytes.length - 1] ^= 1; lines[1] = bytes.toString('base64');
        sig = Buffer.from(lines.join('\n')).toString('base64');
      }
      const tag = channel === 'preview' ? `preview-${sha}` : `v${version}`;
      const manifest = { version, channel: mode === 'wrong-channel' ? 'foreign' : channel, commit: sha, notes: '## Updater acceptance\n\nSigned macOS 0.0.1 → 0.0.2.\n\n' + Array.from({ length: 80 }, (_, i) => `- Release note ${i + 1}: long notes stay inside their scroll area.`).join('\n'), platforms: mode === 'missing-target' ? {} : { [`darwin-${process.arch === 'arm64' ? 'aarch64' : 'x86_64'}`]: { url: `https://github.com/MrFreeZZZ/oxbow/releases/download/${tag}/Oxbow-test.app.tar.gz`, signature: sig } } };
      res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); res.end(JSON.stringify(manifest)); return;
    }
    if (!req.url.startsWith('/MrFreeZZZ/oxbow/releases/download/')) { res.writeHead(404); res.end(); return; }
    const data = Buffer.from(packageBytes);
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
  throw new Error('Usage: node scripts/updater-test.mjs build | serve | mode <scenario> | snapshot | verify');
}
