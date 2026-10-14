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

GitHub Actions CI (verified after the D1 push): run 38560322065 on `main` at `3cb01a7` passed on
`macos-26` (arm64, so the Rust tests also ran natively on an Apple Silicon VM) and `macos-26-intel`,
including `host_facts_are_sane_on_this_machine` on the Intel runner.

### Not verified in D1 (chưa kiểm chứng)

- Apple Silicon runtime: macmon backend, topology mapping from `IODeviceTree:/cpus`, cluster MHz,
  0 W detection on real counters, macOS 14/15/26/27.
- Rosetta mode (x86_64 slice on Apple Silicon): `Unavailable` reason and `hw.perflevel*` clusters;
  RAM page-size behaviour under translation.
- Real Intel Mac (SMC and IOReport on genuine hardware, T2 models).
- Sleep/wake gap handling on real hardware (covered only by unit tests with a fake clock).
- `RunEvent::Exit` shutdown path (no quit menu until D2).

## D2 — popover UI (`feat/popover-ui`)

Date: 2026-10-14. Release x86_64 bundle (`pnpm tauri build --target x86_64-apple-darwin --bundles app`)
started from a terminal with `RUST_LOG=pulsebar=debug`, plus the browser mock (`pnpm dev:mock`)
driven by Playwright (Chromium). The screen was locked for part of the session and the terminal has
no Screen Recording permission, so on-screen checks of the real popover are listed separately below.

### Frontend (mock, Chromium)

| Check                     | Method                                                                                                                    | Result                                                                                            |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| Views render per platform | Apple Silicon (M3 Pro 6E+6P) and Intel (i9-9880H 8C/16T, `smc:PCPT`, nominal) × light/dark × overview/cpu-detail/settings | Pass: 12 screenshots; per-core MHz on Apple Silicon, usage-only grid with a note on nominal Intel |
| Contrast                  | computed for every visible text node, 48 combinations (theme × view × `prefers-contrast` × effects)                       | Pass: minimum 5.38:1                                                                              |
| Keyboard and ARIA         | Tab order, Enter/Space, focus return, Esc → `hide_popover`, progressbar values, labelled switches and radio groups        | Pass                                                                                              |
| Reduced motion            | `prefers-reduced-motion: reduce`                                                                                          | Pass: `--duration-*` become 0 ms; no infinite animations                                          |
| CSP                       | `vite build --mode mock` served with the production CSP header from `tauri.conf.json`                                     | Pass: 0 `securitypolicyviolation` events; dynamic styles use CSSOM only                           |
| Mock data excluded        | `grep -r "i9-9880H" dist/` after `pnpm build`                                                                             | Pass: no match                                                                                    |

### Real app (release x86_64)

| Check                                 | Method                                                                               | Result                                                                                      |
| ------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| Popover pre-created hidden            | debug log at start-up                                                                | Pass: `popover window created`, no window shown                                             |
| Second launch keeps one process       | run the bundle binary again                                                          | Pass: second process exits 0, popover is shown, one `pulsebar` process                      |
| Relaunch through LaunchServices       | `open Pulsebar.app` while running (`RunEvent::Reopen`)                               | Pass: popover shown, still one process, Pulsebar becomes the active app                     |
| Hide on blur                          | `open -a Finder` while the popover is open                                           | Pass: `popover hidden` within 1 s                                                           |
| Settings persist                      | changing settings in the popover                                                     | Pass: `settings.json` written atomically in the app config directory                        |
| Autostart off by default              | `~/Library/LaunchAgents` after first runs                                            | Pass: no Pulsebar plist                                                                     |
| Boot disk vs Disk Utility (data path) | `DiskScanner` probe vs `diskutil info /`                                             | Pass: 74.79 % used vs container (255.85 − 64.49) / 255.85 GB = 74.79 %                      |
| Volume mount detection (data path)    | `hdiutil attach` of an 8 MB test DMG between two scans                               | Pass: next scan reports `changed=true` (0.5 ms); the popover refreshes disks every 2 s tick |
| Wi-Fi IP (data path)                  | `NetworkReader` vs `networksetup -listallhardwareports` and `ipconfig getifaddr en1` | Pass: Wi-Fi is `en1` on this host, same IPv4                                                |
| Uptime (data path)                    | app formula vs `sysctl kern.boottime` and `uptime`                                   | Pass: identical to `now − kern.boottime`                                                    |
| Log noise                             | debug log during the session                                                         | Pass: only show/hide/start-up lines, no warnings                                            |

### Performance (AC9, Intel host)

Measured with `top -l N -s 10 -pid … -stats pid,command,cpu,mem` (MEM = physical footprint) and
`ps -o rss=` for the main process and the app's three WebKit processes (Web Content, Networking,
GPU). The host was busy (load average about 6, RAM nearly full with heavy swapping), which lowers RSS
through compression; footprint is the steadier figure.

| Budget                                     | Measured                                                                     | Status |
| ------------------------------------------ | ---------------------------------------------------------------------------- | ------ |
| CPU (main process), popover closed ≤ 0.8 % | main 0.13 % (first 10 min of a 1 h run; WebKit 0.00 %); 0.22 % over the hour | Pass   |
| CPU (main process), popover open ≤ 3 %     | main 1.74 %; main + WebKit 3.84 % (213 s while visible)                      | Pass   |
| Main footprint ≤ 35 MB                     | footprint 25–33 MB (RSS 30–41 MB closed, 42–43 MB open)                      | Pass   |
| Total footprint ≤ 120 MB                   | footprint 59–73 MB (RSS 89–104 MB closed, 134 MB open)                       | Pass   |
| Main growth < 5 MB after 1 h (closed)      | footprint 31 → 33 MB (+2 MB); RSS 40.6 → 34.4 MB (compression)               | Pass   |

Budget decision (2026-10-14, approved by the owner): the original "main RSS ≤ 25 MB, total ≤ 120 MB"
cannot hold for a WKWebView host with a menu bar status item, so memory budgets are measured as
physical footprint (Activity Monitor's Memory column) with main ≤ 35 MB and total ≤ 120 MB, and the CPU
budgets apply to the main process. The design keeps the pre-created popover for an instant first open.

Note on the main process: about 7.9 MB is a read-only shared IOKit region tied to the menu bar status
item (the same region appears in Finder) and about 10 MB is `IOAccelerator` graphics memory for the
pre-created popover window's surfaces. The open-state CPU was sampled only while the popover was
visible (it was dismissed after 213 s by user interaction).

### Not verified in D2 (chưa kiểm chứng)

- On-screen behaviour of the real popover: placement under the tray icon (also on a second display
  with a different scale), left-click toggle and 200 ms debounce, Esc closing and returning focus to
  the previous app, popover top edge staying put while it resizes, 12 px corners with and without
  "Giảm hiệu ứng", theme switching on the native material, right-click menu items.
- Disk card update in the real WKWebView within 2 s of mounting a volume (the data path is verified).
- CSP inside WKWebView (verified in Chromium with the same header).
- Memory saver releasing and recreating the window after 5 minutes hidden.
- Liquid Glass material (macOS 26+), all Apple Silicon behaviour and budgets.
