import { createHash, createPublicKey, verify } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';

export const DEFAULT_REPO = 'TheHiep0902/macos-system-info';
export const PRODUCT = 'Pulsebar';
export const APP_DIR = `${PRODUCT}.app`;
export const MAX_NOTES_CHARS = 10000;
export const TARGETS = [
  { triple: 'aarch64-apple-darwin', arch: 'aarch64', platform: 'darwin-aarch64', lipo: 'arm64' },
  { triple: 'x86_64-apple-darwin', arch: 'x64', platform: 'darwin-x86_64', lipo: 'x86_64' },
];
export const USAGE =
  'usage: node scripts/release-assets.mjs --version <semver> --commit <40-hex sha> --notes <file> --out <dir> [--repo <owner/name>]';

const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;
const COMMIT = /^[0-9a-f]{40}$/;
const REPO = /^[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?\/[A-Za-z0-9._-]+$/;
const BASE64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
const UNTRUSTED_PREFIX = 'untrusted comment: ';
const TRUSTED_PREFIX = 'trusted comment: ';
const KEY_ALGORITHM = 'Ed';
const PREHASHED_ALGORITHM = 'ED';
const PUBLIC_KEY_BYTES = 42;
const SIGNATURE_BYTES = 74;
const GLOBAL_SIGNATURE_BYTES = 64;
const SAFE_SHELL_WORD = /^[A-Za-z0-9_@%+=:,./-]+$/;
const SAFE_FILE_NAME = /^[A-Za-z0-9._-]+$/;

export class ReleaseError extends Error {
  constructor(message, options) {
    super(message, options);
    this.name = 'ReleaseError';
  }
}

export function isStrictSemver(value) {
  return typeof value === 'string' && SEMVER.test(value);
}

export function isCommitSha(value) {
  return typeof value === 'string' && COMMIT.test(value);
}

export function isRepoSlug(value) {
  return typeof value === 'string' && REPO.test(value);
}

export function decodeBase64(text, what) {
  if (typeof text !== 'string' || text.length === 0 || !BASE64.test(text)) {
    throw new ReleaseError(`${what} is not valid base64`);
  }
  return Buffer.from(text, 'base64');
}

function decodeBase64Text(text, what) {
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(decodeBase64(text, what));
  } catch (error) {
    if (error instanceof ReleaseError) throw error;
    throw new ReleaseError(`${what} does not decode to UTF-8 text`, { cause: error });
  }
}

function contentLines(text) {
  const lines = text.split(/\r?\n/);
  while (lines.length > 0 && lines[lines.length - 1] === '') lines.pop();
  return lines;
}

export function parsePublicKey(encoded) {
  const lines = contentLines(decodeBase64Text(encoded, 'updater pubkey'));
  if (lines.length !== 2 || !lines[0].startsWith(UNTRUSTED_PREFIX)) {
    throw new ReleaseError('updater pubkey is not a minisign public key');
  }
  const bytes = decodeBase64(lines[1], 'updater pubkey key line');
  if (
    bytes.length !== PUBLIC_KEY_BYTES ||
    bytes.subarray(0, 2).toString('latin1') !== KEY_ALGORITHM
  ) {
    throw new ReleaseError('updater pubkey is not an Ed25519 minisign public key');
  }
  return { keyId: bytes.subarray(2, 10), key: bytes.subarray(10) };
}

export function parseSignature(encoded, what) {
  const lines = contentLines(decodeBase64Text(encoded, what));
  if (
    lines.length !== 4 ||
    !lines[0].startsWith(UNTRUSTED_PREFIX) ||
    !lines[2].startsWith(TRUSTED_PREFIX)
  ) {
    throw new ReleaseError(`${what} is not a minisign signature`);
  }
  const bytes = decodeBase64(lines[1], `${what} signature line`);
  const globalSignature = decodeBase64(lines[3], `${what} global signature line`);
  if (bytes.length !== SIGNATURE_BYTES || globalSignature.length !== GLOBAL_SIGNATURE_BYTES) {
    throw new ReleaseError(`${what} has a malformed signature line`);
  }
  return {
    algorithm: bytes.subarray(0, 2).toString('latin1'),
    keyId: bytes.subarray(2, 10),
    signature: bytes.subarray(10),
    trustedComment: lines[2].slice(TRUSTED_PREFIX.length),
    globalSignature,
  };
}

export function signedVersion(trustedComment) {
  const field = trustedComment.split('\t').find((part) => part.startsWith('version:'));
  return field === undefined ? null : field.slice('version:'.length);
}

function ed25519Key(raw) {
  return createPublicKey({
    key: { kty: 'OKP', crv: 'Ed25519', x: Buffer.from(raw).toString('base64url') },
    format: 'jwk',
  });
}

function signedMessage(algorithm, data, what) {
  if (algorithm === PREHASHED_ALGORITHM) return createHash('blake2b512').update(data).digest();
  if (algorithm === KEY_ALGORITHM) return data;
  throw new ReleaseError(`${what} uses an unsupported signature algorithm`);
}

export function verifyUpdaterSignature({ data, signatureText, publicKey, version, what }) {
  const signature = parseSignature(signatureText, what);
  if (!signature.keyId.equals(publicKey.keyId)) {
    throw new ReleaseError(
      `${what} was made with key ${keyIdHex(signature.keyId)}, but the updater pubkey is ${keyIdHex(publicKey.keyId)}`,
    );
  }
  const key = ed25519Key(publicKey.key);
  const message = signedMessage(signature.algorithm, data, what);
  if (!verify(null, message, key, signature.signature)) {
    throw new ReleaseError(`${what} does not match the file it signs`);
  }
  const globalMessage = Buffer.concat([
    signature.signature,
    Buffer.from(signature.trustedComment, 'utf8'),
  ]);
  if (!verify(null, globalMessage, key, signature.globalSignature)) {
    throw new ReleaseError(`${what} has an invalid global signature over its trusted comment`);
  }
  const signed = signedVersion(signature.trustedComment);
  if (signed === null) throw new ReleaseError(`${what} does not carry a signed version`);
  if (signed !== version) {
    throw new ReleaseError(`${what} was signed for version ${signed}, expected ${version}`);
  }
  return signature.trustedComment;
}

export function keyIdHex(keyId) {
  return Buffer.from(keyId).reverse().toString('hex').toUpperCase();
}

export function updaterPubkey(config) {
  const pubkey = config?.plugins?.updater?.pubkey;
  if (typeof pubkey !== 'string' || pubkey.trim() === '') {
    throw new ReleaseError('plugins.updater.pubkey is missing from src-tauri/tauri.conf.json');
  }
  try {
    return parsePublicKey(pubkey.trim());
  } catch (error) {
    throw new ReleaseError(
      `plugins.updater.pubkey in src-tauri/tauri.conf.json is a placeholder or invalid: ${error.message}`,
      { cause: error },
    );
  }
}

export function validateNotes(text) {
  const notes = text.trim();
  if (notes.length === 0) throw new ReleaseError('the notes file is empty');
  const length = [...notes].length;
  if (length > MAX_NOTES_CHARS) {
    throw new ReleaseError(
      `the notes file has ${length} characters, the limit is ${MAX_NOTES_CHARS}`,
    );
  }
  return notes;
}

export function validateTarListing(listing, what) {
  const names = contentLines(listing);
  if (names.length === 0) throw new ReleaseError(`${what} is empty`);
  const roots = new Set();
  for (const name of names) {
    const parts = name.replace(/^\.\//, '').split('/');
    if (name.startsWith('/') || parts.includes('..')) {
      throw new ReleaseError(`${what} contains an unsafe path: ${name}`);
    }
    roots.add(parts[0]);
  }
  if (roots.size !== 1 || !roots.has(APP_DIR)) {
    throw new ReleaseError(
      `${what} must contain exactly one top-level ${APP_DIR}, found: ${[...roots].sort().join(', ')}`,
    );
  }
}

export function assetNames(version, target) {
  return {
    tarball: `${PRODUCT}_${version}_${target.arch}.app.tar.gz`,
    signature: `${PRODUCT}_${version}_${target.arch}.app.tar.gz.sig`,
    dmg: `${PRODUCT}_${version}_${target.arch}.dmg`,
  };
}

export function bundleInputs(root, version, target) {
  const bundle = join(root, 'src-tauri', 'target', target.triple, 'release', 'bundle');
  return {
    app: join(bundle, 'macos', APP_DIR),
    tarball: join(bundle, 'macos', `${APP_DIR}.tar.gz`),
    signature: join(bundle, 'macos', `${APP_DIR}.tar.gz.sig`),
    dmg: join(bundle, 'dmg', assetNames(version, target).dmg),
  };
}

export function rfc3339(date) {
  return date.toISOString().replace(/\.\d{3}Z$/, 'Z');
}

export function buildManifest({ version, notes, commit, repo, pubDate, signatures }) {
  const platforms = {};
  for (const target of TARGETS) {
    platforms[target.platform] = {
      signature: signatures[target.platform],
      url: `https://github.com/${repo}/releases/download/v${version}/${assetNames(version, target).tarball}`,
    };
  }
  const manifest = {
    version,
    notes: `${notes}\n\nCommit: ${commit}`,
    pub_date: pubDate,
    platforms,
  };
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

export function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export function checksumLines(entries) {
  return [...entries]
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
    .map(({ name, digest }) => `${digest}  ${name}\n`)
    .join('');
}

export function shellQuote(word) {
  return SAFE_SHELL_WORD.test(word) ? word : `'${word.replaceAll("'", "'\\''")}'`;
}

export function ghReleaseCommand({ version, commit, repo, notesFile, assets }) {
  return [
    'gh',
    'release',
    'create',
    `v${version}`,
    ...assets,
    '--repo',
    repo,
    '--target',
    commit,
    '--title',
    `${PRODUCT} ${version}`,
    '--notes-file',
    notesFile,
    '--latest',
  ]
    .map(shellQuote)
    .join(' ');
}

export function parseCliArgs(argv) {
  const known = new Set(['--version', '--commit', '--notes', '--out', '--repo']);
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!known.has(flag) || value === undefined || value.startsWith('--')) {
      throw new ReleaseError(USAGE);
    }
    if (flag.slice(2) in values) throw new ReleaseError(`${flag} was given more than once`);
    values[flag.slice(2)] = value;
  }
  for (const flag of ['version', 'commit', 'notes', 'out']) {
    if (!(flag in values)) throw new ReleaseError(USAGE);
  }
  const options = { repo: DEFAULT_REPO, ...values };
  if (!isStrictSemver(options.version)) {
    throw new ReleaseError(`--version ${options.version} is not a strict semver version`);
  }
  if (!isCommitSha(options.commit)) {
    throw new ReleaseError('--commit must be a full 40-character lowercase hex SHA');
  }
  if (!isRepoSlug(options.repo)) throw new ReleaseError(`--repo ${options.repo} is not owner/name`);
  return options;
}

function readText(path, what) {
  try {
    return readFileSync(path, 'utf8');
  } catch (error) {
    throw new ReleaseError(`cannot read ${what} ${path}: ${error.message}`, { cause: error });
  }
}

function readConfig(root) {
  const path = join(root, 'src-tauri', 'tauri.conf.json');
  try {
    return JSON.parse(readText(path, 'config'));
  } catch (error) {
    if (error instanceof ReleaseError) throw error;
    throw new ReleaseError(`cannot parse ${path}: ${error.message}`, { cause: error });
  }
}

function assertCleanCheckout(exec, commit) {
  const head = exec('git', ['rev-parse', 'HEAD']).trim();
  if (head !== commit) throw new ReleaseError(`HEAD is ${head}, expected --commit ${commit}`);
  if (exec('git', ['status', '--porcelain']).trim() !== '') {
    throw new ReleaseError('the working tree is not clean; commit or stash every change first');
  }
}

function assertUsableOut(out) {
  if (!existsSync(out)) return;
  if (!statSync(out).isDirectory())
    throw new ReleaseError(`--out ${out} exists and is not a directory`);
  if (readdirSync(out).length > 0) throw new ReleaseError(`--out ${out} exists and is not empty`);
}

function assertInputs(inputs) {
  for (const [kind, path] of Object.entries(inputs)) {
    if (!existsSync(path)) throw new ReleaseError(`missing ${kind}: ${path}`);
  }
}

function inspectApp(exec, app, version, target, what) {
  const plist = join(app, 'Contents', 'Info.plist');
  const shortVersion = exec('plutil', [
    '-extract',
    'CFBundleShortVersionString',
    'raw',
    plist,
  ]).trim();
  if (shortVersion !== version) {
    throw new ReleaseError(
      `${what} has CFBundleShortVersionString ${shortVersion}, expected ${version}`,
    );
  }
  const executable = exec('plutil', ['-extract', 'CFBundleExecutable', 'raw', plist]).trim();
  if (!SAFE_FILE_NAME.test(executable)) {
    throw new ReleaseError(`${what} has an unexpected CFBundleExecutable: ${executable}`);
  }
  const archs = exec('lipo', ['-archs', join(app, 'Contents', 'MacOS', executable)]).trim();
  if (archs !== target.lipo) {
    throw new ReleaseError(`${what} executable is built for "${archs}", expected "${target.lipo}"`);
  }
}

function inspectTarball(exec, inputs, version, target) {
  const what = `${target.triple} ${APP_DIR}.tar.gz`;
  validateTarListing(exec('tar', ['-tzf', inputs.tarball]), what);
  const scratch = mkdtempSync(join(tmpdir(), 'pulsebar-release-'));
  try {
    exec('tar', ['-xzf', inputs.tarball, '-C', scratch]);
    inspectApp(exec, join(scratch, APP_DIR), version, target, `${what} (${APP_DIR} inside)`);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

function inspectDmg(exec, inputs, version, target) {
  const what = `${target.triple} ${basename(inputs.dmg)}`;
  exec('hdiutil', ['verify', inputs.dmg]);
  const mount = mkdtempSync(join(tmpdir(), 'pulsebar-dmg-'));
  try {
    exec('hdiutil', [
      'attach',
      inputs.dmg,
      '-readonly',
      '-nobrowse',
      '-noautoopen',
      '-mountpoint',
      mount,
    ]);
  } catch (error) {
    rmSync(mount, { recursive: true, force: true });
    throw error;
  }
  try {
    inspectApp(exec, join(mount, APP_DIR), version, target, `${what} (${APP_DIR} inside)`);
  } finally {
    exec('hdiutil', ['detach', mount]);
    rmSync(mount, { recursive: true, force: true });
  }
}

function inspectTarget({ exec, root, version, publicKey }, target) {
  const inputs = bundleInputs(root, version, target);
  assertInputs(inputs);
  inspectApp(exec, inputs.app, version, target, `${target.triple} ${APP_DIR}`);
  inspectTarball(exec, inputs, version, target);
  inspectDmg(exec, inputs, version, target);
  const signatureText = readText(inputs.signature, 'signature').trim();
  verifyUpdaterSignature({
    data: readFileSync(inputs.tarball),
    signatureText,
    publicKey,
    version,
    what: `${target.triple} ${basename(inputs.signature)}`,
  });
  return { target, inputs, signatureText };
}

function writeAssets(staging, { version, commit, repo, notes, now }, checked) {
  const signatures = {};
  const written = [];
  for (const { target, inputs, signatureText } of checked) {
    const names = assetNames(version, target);
    copyFileSync(inputs.tarball, join(staging, names.tarball));
    copyFileSync(inputs.signature, join(staging, names.signature));
    copyFileSync(inputs.dmg, join(staging, names.dmg));
    written.push(names.tarball, names.signature, names.dmg);
    signatures[target.platform] = signatureText;
  }
  const manifest = buildManifest({
    version,
    notes,
    commit,
    repo,
    pubDate: rfc3339(now()),
    signatures,
  });
  writeFileSync(join(staging, 'latest.json'), manifest);
  written.push('latest.json');
  const sums = checksumLines(
    written.map((name) => ({ name, digest: sha256(readFileSync(join(staging, name))) })),
  );
  writeFileSync(join(staging, 'SHA256SUMS'), sums);
  return [...written, 'SHA256SUMS'];
}

export function runRelease({ argv, root, cwd, exec, now }) {
  const options = parseCliArgs(argv);
  const out = resolve(cwd, options.out);
  const notesFile = resolve(cwd, options.notes);
  assertCleanCheckout(exec, options.commit);
  const publicKey = updaterPubkey(readConfig(root));
  assertUsableOut(out);
  const notes = validateNotes(readText(notesFile, 'notes file'));
  const checked = TARGETS.map((target) =>
    inspectTarget({ exec, root, version: options.version, publicKey }, target),
  );
  mkdirSync(dirname(out), { recursive: true });
  const staging = mkdtempSync(join(dirname(out), `.${basename(out)}.partial-`));
  let files;
  try {
    files = writeAssets(staging, { ...options, notes, now }, checked);
    renameSync(staging, out);
  } catch (error) {
    rmSync(staging, { recursive: true, force: true });
    throw error;
  }
  return {
    out,
    files,
    command: ghReleaseCommand({
      version: options.version,
      commit: options.commit,
      repo: options.repo,
      notesFile,
      assets: files.map((name) => join(out, name)),
    }),
  };
}
