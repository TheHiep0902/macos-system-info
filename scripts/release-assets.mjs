import { execFileSync } from 'node:child_process';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ReleaseError, runRelease } from './release-assets-lib.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const MAX_BUFFER = 64 * 1024 * 1024;
const COMMAND_TIMEOUT_MS = 5 * 60 * 1000;

function exec(command, args) {
  try {
    return execFileSync(command, args, {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: MAX_BUFFER,
      timeout: COMMAND_TIMEOUT_MS,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  } catch (error) {
    const detail = (error.stderr || error.message || '').toString().trim().split('\n')[0];
    throw new ReleaseError(`${command} ${args[0]} failed: ${detail}`, { cause: error });
  }
}

try {
  const result = runRelease({
    argv: process.argv.slice(2),
    root,
    cwd: process.cwd(),
    exec,
    now: () => new Date(),
  });
  console.log(`wrote ${result.files.length} files to ${result.out}:`);
  for (const name of result.files) console.log(`  ${name}`);
  console.log('');
  console.log("Publish with (tagging and publishing need the owner's explicit yes first):");
  console.log(result.command);
} catch (error) {
  console.error(error instanceof ReleaseError ? error.message : error);
  process.exitCode = 1;
}
