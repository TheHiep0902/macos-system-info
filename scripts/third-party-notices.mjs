import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  CARGO_ABOUT_VERSION,
  HEADER,
  NoticesError,
  SUPPLEMENTS,
  TARGETS,
  compareCodePoint,
  diffEntries,
  isNoticeFileName,
  missingSupplementBlocks,
  missingSupplementCrates,
  normalClosure,
  packageKey,
  packageNames,
  parseCargoTree,
  parseNoticeEntries,
  renderNotices,
  rootCrateKey,
  supplementLine,
  usesModulePreloadPolyfill,
  usesRolldownRuntime,
  validateNpmEntries,
  viteCoreLicense,
} from './third-party-notices-lib.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const NOTICES_FILE = 'THIRD-PARTY-NOTICES.txt';
const MANIFEST = 'src-tauri/Cargo.toml';
const ABOUT_CONFIG = 'src-tauri/about.toml';
const ABOUT_TEMPLATE = 'src-tauri/about.hbs';
const CARGO_ABOUT = `cargo-about ${CARGO_ABOUT_VERSION}`;
const CARGO_ABOUT_INSTALL = `cargo install --locked --features cli cargo-about@${CARGO_ABOUT_VERSION}`;
const MAX_BUFFER = 64 * 1024 * 1024;
const COMMAND_TIMEOUT_MS = 15 * 60 * 1000;
const USAGE = 'usage: node scripts/third-party-notices.mjs [--check]';

function exec(command, args, stderr) {
  return execFileSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: MAX_BUFFER,
    timeout: COMMAND_TIMEOUT_MS,
    stdio: ['ignore', 'pipe', stderr],
  });
}

function runCommand(command, args) {
  try {
    return exec(command, args, 'inherit');
  } catch (error) {
    throw new NoticesError(`${command} ${args[0]} failed: ${error.message.trim()}`, {
      cause: error,
    });
  }
}

function assertCargoAbout() {
  const hint = `install it with \`${CARGO_ABOUT_INSTALL}\` (see README "Environment setup")`;
  let found;
  try {
    found = exec('cargo', ['about', '--version'], 'pipe').trim();
  } catch (error) {
    throw new NoticesError(`${CARGO_ABOUT} is required (${error.message.trim()}); ${hint}`, {
      cause: error,
    });
  }
  if (found !== CARGO_ABOUT) {
    throw new NoticesError(`${CARGO_ABOUT} is required, found "${found}"; ${hint}`);
  }
}

async function withTempDir(task) {
  const dir = mkdtempSync(join(tmpdir(), 'pulsebar-notices-'));
  try {
    return await task(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) {
    throw new NoticesError(`cannot read ${path}: ${error.message}`, { cause: error });
  }
}

async function buildFrontend(outDir) {
  const { build } = await import('vite');
  try {
    await build({
      root,
      logLevel: 'warn',
      build: { outDir, emptyOutDir: true, license: { fileName: 'licenses.json' } },
    });
  } catch (error) {
    throw new NoticesError(`vite build failed: ${error.message}`, { cause: error });
  }
}

function readBundleScripts(assetsDir) {
  return readdirSync(assetsDir)
    .filter((name) => name.endsWith('.js'))
    .map((name) => readFileSync(join(assetsDir, name), 'utf8'));
}

function viteNpmEntry() {
  const packagePath = createRequire(import.meta.url).resolve('vite/package.json');
  const { version } = readJson(packagePath);
  const licenseMd = readFileSync(join(dirname(packagePath), 'LICENSE.md'), 'utf8');
  return { name: 'vite', version, identifier: 'MIT', text: viteCoreLicense(licenseMd) };
}

async function bundledNpmEntries(outDir) {
  await buildFrontend(outDir);
  const licenses = readJson(join(outDir, 'licenses.json'));
  const scripts = readBundleScripts(join(outDir, 'assets'));
  const errors = validateNpmEntries(licenses);
  if (usesRolldownRuntime(scripts)) {
    errors.push(
      'the bundle contains the __commonJS runtime, whose license is not covered; extend scripts/third-party-notices.mjs to list it',
    );
  }
  failOnErrors(errors);
  return usesModulePreloadPolyfill(scripts) ? [...licenses, viteNpmEntry()] : licenses;
}

function readSupplement(supplement) {
  let bytes;
  try {
    bytes = readFileSync(join(root, supplement.file));
  } catch (error) {
    throw new NoticesError(`cannot read supplement ${supplement.file}: ${error.message}`, {
      cause: error,
    });
  }
  const digest = createHash('sha256').update(bytes).digest('hex');
  return { supplement, text: bytes.toString('utf8'), digest };
}

function supplementDigestErrors(supplements) {
  return supplements
    .filter(({ supplement, digest }) => digest !== supplement.sha256)
    .map(
      ({ supplement, digest }) =>
        `supplement file changed: ${supplement.file} has sha256 ${digest}, expected ${supplement.sha256}`,
    );
}

function staleSupplementErrors(supplements, rust) {
  const listedNames = packageNames(parseNoticeEntries(rust).rust);
  return supplements.flatMap(({ supplement }) =>
    missingSupplementCrates(supplement, listedNames).map(
      (name) =>
        `supplement ${supplement.file} names crate ${name}, which cargo-about no longer lists; update SUPPLEMENTS in scripts/third-party-notices-lib.mjs`,
    ),
  );
}

function failOnErrors(errors) {
  if (errors.length > 0) throw new NoticesError(errors.join('\n'));
}

async function generate() {
  assertCargoAbout();
  const supplements = SUPPLEMENTS.map(readSupplement);
  failOnErrors(supplementDigestErrors(supplements));
  const text = await withTempDir(async (dir) => {
    const rustPath = join(dir, 'rust.txt');
    runCommand('cargo', [
      'about',
      'generate',
      '--locked',
      '--fail',
      '-m',
      MANIFEST,
      '-c',
      ABOUT_CONFIG,
      '-o',
      rustPath,
      ABOUT_TEMPLATE,
    ]);
    const rust = readFileSync(rustPath, 'utf8');
    failOnErrors(staleSupplementErrors(supplements, rust));
    const npmEntries = await bundledNpmEntries(join(dir, 'web'));
    return renderNotices({ header: HEADER, rust, supplements, npmEntries });
  });
  writeFileSync(join(root, NOTICES_FILE), text);
  const listed = parseNoticeEntries(text);
  console.log(
    `wrote ${NOTICES_FILE}: rust ${listed.rust.size} crates, npm ${listed.npm.size} packages`,
  );
}

function readListedEntries() {
  const path = join(root, NOTICES_FILE);
  if (!existsSync(path)) {
    throw new NoticesError(`${NOTICES_FILE} is missing; run \`pnpm notices\` to generate it`);
  }
  const text = readFileSync(path, 'utf8');
  return { text, entries: parseNoticeEntries(text) };
}

function inspectTarget(target) {
  const metadata = JSON.parse(
    runCommand('cargo', [
      'metadata',
      '--locked',
      '--format-version',
      '1',
      '--manifest-path',
      MANIFEST,
      '--filter-platform',
      target,
    ]),
  );
  const linked = parseCargoTree(
    runCommand('cargo', [
      'tree',
      '--locked',
      '--manifest-path',
      MANIFEST,
      '--target',
      target,
      '-e',
      'normal,no-proc-macro',
      '--prefix',
      'none',
      '--format',
      '{p}',
    ]),
  );
  linked.delete(rootCrateKey(metadata));
  return { target, linked, closure: normalClosure(metadata) };
}

function listCrateDir(key, dir) {
  try {
    return readdirSync(dir, { withFileTypes: true });
  } catch (error) {
    throw new NoticesError(`cannot read the source of crate ${key} at ${dir}: ${error.message}`, {
      cause: error,
    });
  }
}

function noticeFileErrors(closure) {
  return [...closure.entries()]
    .sort(([a], [b]) => compareCodePoint(a, b))
    .flatMap(([key, { dir }]) =>
      listCrateDir(key, dir)
        .filter((entry) => !entry.isDirectory() && isNoticeFileName(entry.name))
        .map((entry) => entry.name)
        .sort(compareCodePoint)
        .map((name) => `crate ships a NOTICE file: ${key} (${name})`),
    );
}

function perTargetCounts(targets, pick) {
  return targets.map((info) => `${info.target.split('-')[0]} ${pick(info).size}`).join(', ');
}

async function check() {
  const { text: noticesText, entries: listed } = readListedEntries();
  const supplements = SUPPLEMENTS.map(readSupplement);
  const targets = TARGETS.map(inspectTarget);
  const linked = new Set(targets.flatMap((info) => [...info.linked]));
  const closure = new Map(targets.flatMap((info) => [...info.closure]));
  const npmEntries = await withTempDir((dir) => bundledNpmEntries(join(dir, 'web')));
  const npmExpected = new Set(npmEntries.map((entry) => packageKey(entry.name, entry.version)));
  const rust = diffEntries({ required: linked, allowed: closure, actual: listed.rust });
  const npm = diffEntries({ required: npmExpected, allowed: npmExpected, actual: listed.npm });
  const errors = [
    ...rust.missing.map((key) => `missing crate: ${key}`),
    ...rust.extra.map((key) => `stale crate: ${key}`),
    ...npm.missing.map((key) => `missing npm package: ${key}`),
    ...npm.extra.map((key) => `stale npm package: ${key}`),
    ...noticeFileErrors(closure),
    ...supplementDigestErrors(supplements),
    ...missingSupplementBlocks(SUPPLEMENTS, noticesText).map(
      (supplement) => `missing supplementary text: ${supplementLine(supplement)}`,
    ),
  ];
  for (const error of errors) console.error(error);
  console.log(
    `rust ${listed.rust.size} listed, linked ${linked.size} (${perTargetCounts(targets, (info) => info.linked)}), lockfile closure ${closure.size} (${perTargetCounts(targets, (info) => info.closure)}), npm ${listed.npm.size}`,
  );
  if (errors.length > 0) process.exitCode = 1;
}

async function main(args) {
  if (args.length === 0) return generate();
  if (args.length === 1 && args[0] === '--check') return check();
  console.error(USAGE);
  process.exitCode = 2;
}

try {
  await main(process.argv.slice(2));
} catch (error) {
  console.error(error instanceof NoticesError ? error.message : error);
  process.exitCode = 1;
}
