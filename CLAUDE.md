# Pulsebar project rules

Rules for this repository. The global rules in `~/.claude/CLAUDE.md` still apply and win on conflict.

## Clean build outputs after each deliverable

After a deliverable is pushed and its CI run has been checked once, remove the local build outputs
and test artifacts so the disk does not fill up (`src-tauri/target/` alone grows past 10 GB):

1. Detach any test volume (`hdiutil detach`) and stop any Pulsebar process started for testing.
2. `cargo clean --manifest-path src-tauri/Cargo.toml` — removes `src-tauri/target/`, including every
   release and debug bundle.
3. `rm -rf dist node_modules/.vite`.
4. Delete the test artifacts written for the deliverable: Playwright output under
   `/tmp/playwright-mcp/pulsebar/` and the session scratchpad (test DMGs, logs, screenshots).

Never remove:

- `node_modules/` (dependencies), `.codegraph/`, or any tracked file;
- the owner's data in `~/Library/Application Support/com.thehiep.pulsebar/`, including its backups;
- global caches (`~/.cargo/registry`, the pnpm store, `~/Library/Caches`): report their size and give
  the commands for the owner to run instead.

Skip or postpone the cleanup while a bundle is still needed (the owner is testing it, or a
measurement runs on it). The first build after a cleanup is a full rebuild.

Save Playwright screenshots and traces under `/tmp/playwright-mcp/pulsebar/` so the cleanup never
touches files of other projects.
