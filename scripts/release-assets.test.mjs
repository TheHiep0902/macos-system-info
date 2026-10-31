import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  APP_DIR,
  DEFAULT_REPO,
  MAX_NOTES_CHARS,
  ReleaseError,
  TARGETS,
  buildManifest,
  checksumLines,
  ghReleaseCommand,
  isStrictSemver,
  keyIdHex,
  parseCliArgs,
  parsePublicKey,
  runRelease,
  signedVersion,
  updaterPubkey,
  validateNotes,
  validateTarListing,
  verifyUpdaterSignature,
} from './release-assets-lib.mjs';

const VERSION = '0.3.0';
const COMMIT = 'a'.repeat(40);
const KEY_ID = Buffer.from('0123456789abcdef', 'hex');
const FIXED_NOW = new Date('2026-10-31T08:09:10.123Z');
const TAURI_PUBLIC_KEY =
  'untrusted comment: minisign public key: A49EF4746FA6E6D7\nRWTX5qZvdPSepDGWg2bz8n6ArQnBMwgQK1geVosorcbLzcunD+KJ+cYe';
const TAURI_SIGNATURE =
  'untrusted comment: signature from tauri secret key\nRUTX5qZvdPSepPF2zsjvP/L/TQtBLRnWRvaq+4omK1UEugtQp/USrGiI1v762sDy3id3aor7bRC9ETyeot6lMHHrWoZ5hjUM1Qc=\ntrusted comment: timestamp:1791649051\tfile:payload\tversion:2.0.0\nDDq2CLOEl5cQmC2eJRHlnZPVViOZsu1CjpPKQoei4A/XBfkDqGO0B+QMrcrHgEQ5qV3mne75tdFWhJpqUXh9AA==';
const MINISIGN_PUBLIC_KEY =
  'untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3';
const MINISIGN_SIGNATURE =
  'untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==';

const b64 = (value) => Buffer.from(value).toString('base64');

function keyPair(keyId = KEY_ID) {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const raw = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url');
  const text = `untrusted comment: minisign public key: ${keyIdHex(keyId)}\n${b64(Buffer.concat([Buffer.from('Ed'), keyId, raw]))}\n`;
  return { keyId, privateKey, pubkey: b64(text) };
}

function signFile(pair, data, options = {}) {
  const algorithm = options.algorithm ?? 'ED';
  const trusted =
    options.trusted ??
    `timestamp:1790000000\tfile:${APP_DIR}.tar.gz\tversion:${options.version ?? VERSION}`;
  const message = algorithm === 'ED' ? createHash('blake2b512').update(data).digest() : data;
  const signature = sign(null, message, pair.privateKey);
  const global = sign(null, Buffer.concat([signature, Buffer.from(trusted)]), pair.privateKey);
  const signedComment = options.tamperComment ?? trusted;
  const text = [
    'untrusted comment: signature from tauri secret key',
    b64(Buffer.concat([Buffer.from(algorithm), options.keyId ?? pair.keyId, signature])),
    `trusted comment: ${signedComment}`,
    b64(global),
    '',
  ].join('\n');
  return b64(text);
}

function verifyWith(pair, data, signatureText, version = VERSION) {
  return verifyUpdaterSignature({
    data,
    signatureText,
    publicKey: parsePublicKey(pair.pubkey),
    version,
    what: 'test.sig',
  });
}

describe('minisign verification', () => {
  const pair = keyPair();
  const data = Buffer.from('tarball bytes');

  it('accepts a prehashed signature made the way tauri build makes it', () => {
    expect(verifyWith(pair, data, signFile(pair, data))).toBe(
      `timestamp:1790000000\tfile:${APP_DIR}.tar.gz\tversion:${VERSION}`,
    );
  });

  it('accepts the legacy non-prehashed Ed algorithm, as the plugin does', () => {
    expect(() => verifyWith(pair, data, signFile(pair, data, { algorithm: 'Ed' }))).not.toThrow();
  });

  it('rejects a tampered file', () => {
    expect(() => verifyWith(pair, Buffer.from('tarball bytez'), signFile(pair, data))).toThrow(
      /does not match the file/,
    );
  });

  it('rejects a signature whose key id differs from the pubkey', () => {
    const other = Buffer.from('fedcba9876543210', 'hex');
    expect(() => verifyWith(pair, data, signFile(pair, data, { keyId: other }))).toThrow(
      /made with key 1032547698BADCFE, but the updater pubkey is EFCDAB8967452301/,
    );
  });

  it('rejects a signature from another key with the same key id', () => {
    const impostor = keyPair();
    expect(() => verifyWith(pair, data, signFile(impostor, data))).toThrow(/does not match/);
  });

  it('rejects a tampered trusted comment through the global signature', () => {
    const tampered = signFile(pair, data, {
      tamperComment: `timestamp:1790000000\tfile:${APP_DIR}.tar.gz\tversion:9.9.9`,
    });
    expect(() => verifyWith(pair, data, tampered, '9.9.9')).toThrow(/invalid global signature/);
  });

  it('rejects a trusted comment signed for another version', () => {
    expect(() => verifyWith(pair, data, signFile(pair, data, { version: '0.2.9' }))).toThrow(
      /signed for version 0\.2\.9, expected 0\.3\.0/,
    );
  });

  it('rejects a signature without a signed version', () => {
    const unversioned = signFile(pair, data, { trusted: 'timestamp:1\tfile:x' });
    expect(() => verifyWith(pair, data, unversioned)).toThrow(/does not carry a signed version/);
  });

  it('rejects an unknown algorithm and malformed encodings', () => {
    expect(() => verifyWith(pair, data, signFile(pair, data, { algorithm: 'XX' }))).toThrow(
      /unsupported signature algorithm/,
    );
    expect(() => verifyWith(pair, data, 'not base64!')).toThrow(/not valid base64/);
    expect(() => verifyWith(pair, data, b64('only one line'))).toThrow(/not a minisign signature/);
  });

  it('verifies the real tauri signer vector from tauri-plugin-updater 2.14.1', () => {
    const result = verifyUpdaterSignature({
      data: Buffer.from('test'),
      signatureText: b64(TAURI_SIGNATURE),
      publicKey: parsePublicKey(b64(TAURI_PUBLIC_KEY)),
      version: '2.0.0',
      what: 'vector',
    });
    expect(signedVersion(result)).toBe('2.0.0');
  });

  it('verifies the minisign-verify vector but refuses it for lacking a version', () => {
    expect(() =>
      verifyUpdaterSignature({
        data: Buffer.from('test'),
        signatureText: b64(MINISIGN_SIGNATURE),
        publicKey: parsePublicKey(b64(MINISIGN_PUBLIC_KEY)),
        version: '2.0.0',
        what: 'vector',
      }),
    ).toThrow(/does not carry a signed version/);
    expect(() =>
      verifyUpdaterSignature({
        data: Buffer.from('tesT'),
        signatureText: b64(MINISIGN_SIGNATURE),
        publicKey: parsePublicKey(b64(MINISIGN_PUBLIC_KEY)),
        version: '2.0.0',
        what: 'vector',
      }),
    ).toThrow(/does not match/);
  });

  it('reads only an exact version field like the plugin', () => {
    expect(signedVersion('timestamp:1\tfile:a\tversion:1.2.3')).toBe('1.2.3');
    expect(signedVersion('timestamp:1\tfile:app-version:2.zip')).toBeNull();
  });
});

describe('updater pubkey', () => {
  it('reads plugins.updater.pubkey', () => {
    const pair = keyPair();
    expect(updaterPubkey({ plugins: { updater: { pubkey: pair.pubkey } } }).keyId).toEqual(KEY_ID);
  });

  it('refuses a missing, placeholder or undecodable pubkey', () => {
    expect(() => updaterPubkey({})).toThrow(/missing/);
    expect(() => updaterPubkey({ plugins: { updater: { pubkey: '  ' } } })).toThrow(/missing/);
    expect(() =>
      updaterPubkey({ plugins: { updater: { pubkey: 'REPLACE_WITH_PUBKEY' } } }),
    ).toThrow(/placeholder or invalid/);
    expect(() =>
      updaterPubkey({ plugins: { updater: { pubkey: b64('untrusted comment: x\nAAAA\n') } } }),
    ).toThrow(/placeholder or invalid/);
  });
});

describe('input validation', () => {
  it('accepts strict semver only', () => {
    expect(isStrictSemver('0.3.0')).toBe(true);
    expect(isStrictSemver('1.0.0-rc.1+build.5')).toBe(true);
    expect(isStrictSemver('v0.3.0')).toBe(false);
    expect(isStrictSemver('0.3')).toBe(false);
    expect(isStrictSemver('01.0.0')).toBe(false);
  });

  it('parses arguments and refuses bad ones', () => {
    const base = ['--version', VERSION, '--commit', COMMIT, '--notes', 'n.md', '--out', 'out'];
    expect(parseCliArgs(base)).toEqual({
      version: VERSION,
      commit: COMMIT,
      notes: 'n.md',
      out: 'out',
      repo: DEFAULT_REPO,
    });
    expect(() => parseCliArgs(base.slice(0, 6))).toThrow(/usage/);
    expect(() => parseCliArgs([...base, '--bogus', 'x'])).toThrow(/usage/);
    expect(() => parseCliArgs([...base, '--version', VERSION])).toThrow(/more than once/);
    expect(() => parseCliArgs(['--version', '0.3', ...base.slice(2)])).toThrow(/strict semver/);
    expect(() => parseCliArgs([...base.slice(0, 2), '--commit', 'abc', ...base.slice(4)])).toThrow(
      /40-character/,
    );
    expect(() => parseCliArgs([...base, '--repo', 'a/b;rm'])).toThrow(/owner\/name/);
  });

  it('refuses empty and oversized notes', () => {
    expect(validateNotes('  hello \n')).toBe('hello');
    expect(() => validateNotes(' \n ')).toThrow(/empty/);
    expect(validateNotes('x'.repeat(MAX_NOTES_CHARS))).toHaveLength(MAX_NOTES_CHARS);
    expect(() => validateNotes('x'.repeat(MAX_NOTES_CHARS + 1))).toThrow(/limit is 10000/);
  });

  it('requires exactly one top-level app in the tarball', () => {
    expect(() => validateTarListing(`${APP_DIR}/\n${APP_DIR}/Contents/\n`, 't')).not.toThrow();
    expect(() => validateTarListing(`./${APP_DIR}/Contents/Info.plist\n`, 't')).not.toThrow();
    expect(() => validateTarListing(`${APP_DIR}/\nextra.txt\n`, 't')).toThrow(/exactly one/);
    expect(() => validateTarListing('Other.app/\n', 't')).toThrow(/exactly one/);
    expect(() => validateTarListing('', 't')).toThrow(/empty/);
    expect(() => validateTarListing(`${APP_DIR}/../evil\n`, 't')).toThrow(/unsafe path/);
    expect(() => validateTarListing('/etc/passwd\n', 't')).toThrow(/unsafe path/);
  });
});

describe('outputs', () => {
  it('builds the manifest with exactly the expected shape', () => {
    const text = buildManifest({
      version: VERSION,
      notes: 'Fixes.',
      commit: COMMIT,
      repo: DEFAULT_REPO,
      pubDate: '2026-10-31T08:09:10Z',
      signatures: { 'darwin-aarch64': 'SIGA', 'darwin-x86_64': 'SIGX' },
    });
    expect(text.endsWith('}\n')).toBe(true);
    expect(text).toContain('\n  "version": "0.3.0",\n');
    expect(JSON.parse(text)).toEqual({
      version: VERSION,
      notes: `Fixes.\n\nCommit: ${COMMIT}`,
      pub_date: '2026-10-31T08:09:10Z',
      platforms: {
        'darwin-aarch64': {
          signature: 'SIGA',
          url: `https://github.com/${DEFAULT_REPO}/releases/download/v0.3.0/Pulsebar_0.3.0_aarch64.app.tar.gz`,
        },
        'darwin-x86_64': {
          signature: 'SIGX',
          url: `https://github.com/${DEFAULT_REPO}/releases/download/v0.3.0/Pulsebar_0.3.0_x64.app.tar.gz`,
        },
      },
    });
  });

  it('formats SHA256SUMS sorted by name with two spaces', () => {
    expect(
      checksumLines([
        { name: 'b.dmg', digest: '2'.repeat(64) },
        { name: 'a.json', digest: '1'.repeat(64) },
      ]),
    ).toBe(`${'1'.repeat(64)}  a.json\n${'2'.repeat(64)}  b.dmg\n`);
  });

  it('quotes the printed gh command', () => {
    expect(
      ghReleaseCommand({
        version: VERSION,
        commit: COMMIT,
        repo: DEFAULT_REPO,
        notesFile: "/tmp/my notes's.md",
        assets: ['/o/a.dmg'],
      }),
    ).toBe(
      `gh release create v0.3.0 /o/a.dmg --repo ${DEFAULT_REPO} --target ${COMMIT} --title 'Pulsebar 0.3.0' --notes-file '/tmp/my notes'\\''s.md' --latest`,
    );
  });
});

const temps = [];

function tempDir() {
  const dir = mkdtempSync(join(tmpdir(), 'pulsebar-release-test-'));
  temps.push(dir);
  return dir;
}

afterEach(() => {
  while (temps.length > 0) rmSync(temps.pop(), { recursive: true, force: true });
});

function writeApp(app, { version = VERSION, executable = 'pulsebar', arch }) {
  mkdirSync(join(app, 'Contents', 'MacOS'), { recursive: true });
  writeFileSync(
    join(app, 'Contents', 'Info.plist'),
    JSON.stringify({ CFBundleShortVersionString: version, CFBundleExecutable: executable }),
  );
  writeFileSync(join(app, 'Contents', 'MacOS', executable), arch);
}

function fixture(overrides = {}) {
  const root = tempDir();
  const work = tempDir();
  const pair = keyPair();
  mkdirSync(join(root, 'src-tauri'), { recursive: true });
  writeFileSync(
    join(root, 'src-tauri', 'tauri.conf.json'),
    JSON.stringify({ plugins: { updater: { pubkey: overrides.pubkey ?? pair.pubkey } } }),
  );
  const tarballs = new Map();
  for (const target of TARGETS) {
    const bundle = join(root, 'src-tauri', 'target', target.triple, 'release', 'bundle');
    const arch = overrides.arch?.[target.triple] ?? target.lipo;
    writeApp(join(bundle, 'macos', APP_DIR), { arch, version: overrides.plistVersion });
    const tarball = join(bundle, 'macos', `${APP_DIR}.tar.gz`);
    const data = Buffer.from(`tarball ${target.triple}`);
    writeFileSync(tarball, data);
    writeFileSync(
      `${tarball}.sig`,
      overrides.signature?.(pair, data, target) ?? signFile(pair, data),
    );
    mkdirSync(join(bundle, 'dmg'), { recursive: true });
    writeFileSync(
      join(bundle, 'dmg', `Pulsebar_${VERSION}_${target.arch}.dmg`),
      `dmg ${target.arch}`,
    );
    tarballs.set(tarball, {
      listing: overrides.listing ?? `${APP_DIR}/\n${APP_DIR}/Contents/\n`,
      arch: overrides.tarArch?.[target.triple] ?? arch,
    });
  }
  writeFileSync(join(work, 'notes.md'), overrides.notes ?? 'Adds the in-app updater.\n');
  const calls = [];
  const exec = (command, args) => {
    calls.push([command, ...args]);
    if (command === 'git' && args[0] === 'rev-parse') return `${overrides.head ?? COMMIT}\n`;
    if (command === 'git' && args[0] === 'status') return overrides.status ?? '';
    if (command === 'plutil') return `${JSON.parse(readFileSync(args[3], 'utf8'))[args[1]]}\n`;
    if (command === 'lipo') return `${readFileSync(args[1], 'utf8')}\n`;
    if (command === 'tar' && args[0] === '-tzf') return tarballs.get(args[1]).listing;
    if (command === 'hdiutil' && args[0] === 'verify') return '';
    if (command === 'hdiutil' && args[0] === 'attach') {
      const triple = TARGETS.find((target) => args[1].endsWith(`_${target.arch}.dmg`)).triple;
      writeApp(join(args.at(-1), APP_DIR), {
        arch:
          overrides.dmgArch?.[triple] ??
          overrides.arch?.[triple] ??
          TARGETS.find((target) => target.triple === triple).lipo,
        version: overrides.dmgVersion ?? overrides.plistVersion,
      });
      return '';
    }
    if (command === 'hdiutil' && args[0] === 'detach') return '';
    if (command === 'tar' && args[0] === '-xzf') {
      writeApp(join(args[3], APP_DIR), { arch: tarballs.get(args[1]).arch });
      return '';
    }
    throw new Error(`unexpected command ${command} ${args.join(' ')}`);
  };
  const argv = [
    '--version',
    VERSION,
    '--commit',
    COMMIT,
    '--notes',
    'notes.md',
    '--out',
    overrides.out ?? 'release',
  ];
  const run = (now = () => FIXED_NOW) => runRelease({ argv, root, cwd: work, exec, now });
  return { root, work, pair, calls, run, out: join(work, overrides.out ?? 'release') };
}

describe('runRelease', () => {
  it('writes every asset, the manifest and checksums', () => {
    const { run, out, root, calls } = fixture();
    const result = run();
    const names = [
      'Pulsebar_0.3.0_aarch64.app.tar.gz',
      'Pulsebar_0.3.0_aarch64.app.tar.gz.sig',
      'Pulsebar_0.3.0_aarch64.dmg',
      'Pulsebar_0.3.0_x64.app.tar.gz',
      'Pulsebar_0.3.0_x64.app.tar.gz.sig',
      'Pulsebar_0.3.0_x64.dmg',
      'latest.json',
    ];
    expect(readdirSync(out).sort()).toEqual([...names, 'SHA256SUMS'].sort());
    const macos = join(
      root,
      'src-tauri',
      'target',
      'x86_64-apple-darwin',
      'release',
      'bundle',
      'macos',
    );
    expect(readFileSync(join(out, 'Pulsebar_0.3.0_x64.app.tar.gz.sig'))).toEqual(
      readFileSync(join(macos, `${APP_DIR}.tar.gz.sig`)),
    );
    const manifest = JSON.parse(readFileSync(join(out, 'latest.json'), 'utf8'));
    expect(manifest.pub_date).toBe('2026-10-31T08:09:10Z');
    expect(manifest.notes).toBe(`Adds the in-app updater.\n\nCommit: ${COMMIT}`);
    expect(manifest.platforms['darwin-x86_64'].signature).toBe(
      readFileSync(join(macos, `${APP_DIR}.tar.gz.sig`), 'utf8').trim(),
    );
    const sums = readFileSync(join(out, 'SHA256SUMS'), 'utf8').trim().split('\n');
    expect(sums.map((line) => line.split('  ')[1])).toEqual([...names].sort());
    for (const line of sums) {
      const [digest, name] = line.split('  ');
      expect(digest).toBe(
        createHash('sha256')
          .update(readFileSync(join(out, name)))
          .digest('hex'),
      );
    }
    expect(result.command).toContain(`gh release create v0.3.0 ${join(out, names[0])}`);
    expect(result.command).toContain(`--target ${COMMIT} --title 'Pulsebar 0.3.0'`);
    expect(calls.filter(([command]) => command === 'lipo')).toHaveLength(6);
    const hdiutil = calls.filter(([command]) => command === 'hdiutil').map(([, verb]) => verb);
    expect(hdiutil).toEqual(['verify', 'attach', 'detach', 'verify', 'attach', 'detach']);
  });

  it('accepts an existing empty out dir', () => {
    const { run, out } = fixture();
    mkdirSync(out);
    run();
    expect(readdirSync(out)).toHaveLength(8);
  });

  const refusals = [
    ['HEAD differs from --commit', { head: 'b'.repeat(40) }, /HEAD is b+, expected/],
    ['the tree is dirty', { status: ' M src/App.svelte\n' }, /not clean/],
    ['the pubkey is a placeholder', { pubkey: 'PLACEHOLDER' }, /placeholder or invalid/],
    ['the plist version differs', { plistVersion: '0.2.0' }, /CFBundleShortVersionString 0\.2\.0/],
    [
      'the app has the wrong arch',
      { arch: { 'x86_64-apple-darwin': 'arm64' } },
      /expected "x86_64"/,
    ],
    [
      'the app is universal',
      { arch: { 'aarch64-apple-darwin': 'x86_64 arm64' } },
      /expected "arm64"/,
    ],
    [
      'the tarball app has the wrong arch',
      { tarArch: { 'aarch64-apple-darwin': 'x86_64' } },
      /inside/,
    ],
    [
      'the dmg app has another version',
      { dmgVersion: '0.2.9' },
      /dmg \(Pulsebar\.app inside\).*0\.2\.9/,
    ],
    [
      'the dmg app has the wrong arch',
      { dmgArch: { 'x86_64-apple-darwin': 'arm64' } },
      /x64\.dmg \(Pulsebar\.app inside\).*expected "x86_64"/,
    ],
    ['the tarball has extra roots', { listing: `${APP_DIR}/\nREADME\n` }, /exactly one/],
    ['the notes are empty', { notes: '\n' }, /notes file is empty/],
    [
      'a signature is for another version',
      { signature: (pair, data) => signFile(pair, data, { version: '0.2.0' }) },
      /signed for version 0\.2\.0/,
    ],
    [
      'a signature is for another file',
      { signature: (pair) => signFile(pair, Buffer.from('other')) },
      /does not match the file/,
    ],
  ];

  it.each(refusals)('refuses when %s and writes nothing', (_, overrides, message) => {
    const { run, out, work } = fixture(overrides);
    expect(run).toThrow(message);
    expect(existsSync(out)).toBe(false);
    expect(readdirSync(work)).toEqual(['notes.md']);
  });

  it('refuses a missing input', () => {
    const { run, root, out } = fixture();
    rmSync(join(root, 'src-tauri', 'target', 'x86_64-apple-darwin', 'release', 'bundle', 'dmg'), {
      recursive: true,
    });
    expect(run).toThrow(/missing dmg: .*Pulsebar_0\.3\.0_x64\.dmg/);
    expect(existsSync(out)).toBe(false);
  });

  it('refuses a non-empty out dir and leaves it untouched', () => {
    const { run, out } = fixture();
    mkdirSync(out);
    writeFileSync(join(out, 'keep.txt'), 'keep');
    expect(run).toThrow(/exists and is not empty/);
    expect(readdirSync(out)).toEqual(['keep.txt']);
  });

  it('removes the partial output when a late step fails', () => {
    const { run, out, work } = fixture();
    expect(() =>
      run(() => {
        throw new ReleaseError('clock failed');
      }),
    ).toThrow(/clock failed/);
    expect(existsSync(out)).toBe(false);
    expect(readdirSync(work)).toEqual(['notes.md']);
  });
});
