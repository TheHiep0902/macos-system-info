# Verification log

Manual and automated checks per deliverable. Anything that could not be run on the
development host is marked **chưa kiểm chứng** (not verified).

Development host: Hackintosh, Intel Core i7-7700HQ (4C/8T), macOS 14.8.9, VirtualSMC 1.3.8
(SMC values are emulated from RAPL, so they are only indicative of a real Intel Mac).
No Apple Silicon machine is available locally.

## D1 — core sampler (`feat/core-sampler`)

Date: 2026-10-14. App started with `RUST_LOG=pulsebar=trace pnpm tauri dev` (debug build).

| Check                                       | Method                                                                | Result                                                                                 |
| ------------------------------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| Title format and 2 s cadence                | `title …` trace lines with wall-clock timestamps                      | Pass: `CPU 44% · ␣23W · RAM 71%` updated every 2 s (only re-set when the text changes) |
| CPU % vs `top` (background load)            | 30 s window, `top -l 2 -s 30` vs mean of titles                       | Pass: 33.6 % vs 32.7 % (Δ 0.9)                                                         |
| CPU % vs `top` (`yes > /dev/null` × 8)      | 20 s window                                                           | Pass: 89.8 % vs 90.6 % (Δ 0.8)                                                         |
| RAM % vs "Memory Used"                      | `vm_stat` (anonymous − purgeable + wired + compressor) / `hw.memsize` | Pass: 70.7–71.5 % vs title 71 %                                                        |
| Power rises under load                      | titles idle vs `yes` × 8                                              | Pass: 15–23 W → 35–38 W                                                                |
| Power vs SMC `PCPT` (independent read path) | `cargo run --example probe_intel` during load                         | Pass: SMC 34.6–37.9 W (mean ≈ 36.4 W) vs title 35–38 W (mean ≈ 36.6 W)                 |
| IOReport vs SMC per sample                  | `probe_intel`                                                         | IOReport package 34.4–37.8 W vs PCPT 34.6–37.9 W; VirtualSMC lags about one 2 s tick   |
| No Dock icon                                | `lsappinfo info -only ApplicationType -app pulsebar`                  | Pass: `UIElement`                                                                      |
| No child processes                          | `pgrep -P <pid>` after 5 min 37 s                                     | Pass: empty                                                                            |
| Single instance                             | start `target/debug/pulsebar` a second time                           | Pass: second process exits, original keeps running                                     |
| Warnings at default level                   | grep for WARN, ERROR or panicked in the run log                       | Pass: none                                                                             |
| Monospaced digit font on the title          | warning is logged if the status item button is missing; none logged   | Visual check **chưa kiểm chứng**                                                       |

Optional cross-check not run: `sudo powermetrics --samplers cpu_power -i 2000 -n 5` (needs sudo; for the
user to run).

Automated gates at the end of D1: see the commit proposal (cargo fmt/clippy for both targets,
`cargo test` on x86_64, `cargo test --no-run` on aarch64, pnpm format/lint/check/test/build).

### Not verified in D1 (chưa kiểm chứng)

- Apple Silicon runtime: macmon backend, topology mapping from `IODeviceTree:/cpus`, cluster MHz,
  0 W detection on real counters, macOS 14/15/26/27.
- Rosetta mode (x86_64 slice on Apple Silicon): `Unavailable` reason and `hw.perflevel*` clusters;
  RAM page-size behaviour under translation.
- Real Intel Mac (SMC and IOReport on genuine hardware, T2 models).
- Sleep/wake gap handling on real hardware (covered only by unit tests with a fake clock).
- `RunEvent::Exit` shutdown path (no quit menu until D2).
- GitHub Actions CI (no remote yet); `host_facts_are_sane_on_this_machine` assumes
  `hw.cpufrequency` exists on the `macos-26-intel` runner.
