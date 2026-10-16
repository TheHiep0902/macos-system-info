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

### On-screen checks of the real popover (V0)

Date: 2026-10-15 (no code change). Release x86_64 rebuilt from a clean `main` at `ed7b7d0`
(`pnpm tauri build --target x86_64-apple-darwin --bundles app`), plus a debug x86_64 build for the CSP
check (`--debug`). One display, 1920×1080 at 1x, macOS 14.8.9, dark appearance, the owner's settings
(compact title, "Giảm hiệu ứng" on). No computer-use tool was available, so the checks were automated
with small Swift helpers: `CGWindowListCopyWindowInfo` for window bounds (layer 25 = status item,
5 = popover, 101 = context menu), CGEvent mouse clicks and key presses (no typed text),
`screencapture` for pixels, `NSWorkspace.frontmostApplication` for focus and the app's debug log.
Permissions (Accessibility, Screen Recording, event posting) were only read, never requested.

| #   | Check                                    | Method                                                                                                                  | Result                                                                                                                                                                                                                                                                      |
| --- | ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Popover under the icon                   | `CGWindowList` bounds of the status item and the popover                                                                | Pass: icon x = 1149, w = 139 (centre 1218.5); popover x = 1039, w = 360 (centre 1219); top y = 29 (menu bar 24 + gap). Second display with another scale: **chưa kiểm chứng** (single display)                                                                              |
| 2   | Left-click toggle and 200 ms debounce    | CGEvent clicks; blur with `open -a Finder`; one click fired from a log watcher                                          | Pass: click while open → `popover hidden`, focus back to the previous app; click again → shown. A click 31 ms after blur logs `tray click ignored right after the popover lost focus` and the popover stays hidden; a click 265 ms after blur reopens it                    |
| 3   | Esc, immediate reopen, focus return      | CGEvent Esc; frontmost app before and after                                                                             | Pass: Esc hides the popover and the previous app (Claude) becomes frontmost; clicks 313 ms and 1.3 s after Esc reopen it with Pulsebar frontmost                                                                                                                            |
| 4   | Top edge fixed while resizing            | window bounds while switching views                                                                                     | Pass: overview → settings height 640 → 566, CPU detail 418; y stays 29, x unchanged                                                                                                                                                                                         |
| 5   | 12 px corners, theme on the material     | `screencapture -l <window> -o` (alpha); radius from transparent coverage r = √(n / (1 − π/4)) over a 24 px corner block | Pass: effects reduced r ≈ 12.0 px on all 4 corners; native material r ≈ 11.5 px (material mask, ±0.5 px antialiasing). Light / Dark / System switch instantly on the material                                                                                               |
| 6   | Right-click menu                         | screenshot of the menu                                                                                                  | Pass: Mở Pulsebar · Khởi động cùng macOS (unchecked = off) · Cài đặt… · separator · Thoát. "Cài đặt…" opens the popover on the settings view; "Thoát" quits                                                                                                                 |
| 7   | Disk card within 2 s of `hdiutil attach` | 20 MB APFS test DMG; frame diff of the card region against a 20 ms `mount` poller                                       | Pass: "PulsebarV0 · Ổ rời" appears ≤ 1.58 s after the volume mounted (another run ≤ 0.45 s after the command); removed ≤ 0.35 s after unmount                                                                                                                               |
| 8   | CSP inside WKWebView                     | debug build, popover right-click → Inspect Element; Network and Console after "Reload"                                  | Pass: the document response carries `Content-Security-Policy: script-src 'self' 'sha256-…'; style-src 'self'; default-src 'self' ipc: http://ipc.localhost; img-src 'self' data:`; console empty after reload and overview → CPU detail → settings                          |
| 9   | Memory saver                             | memory saver on, popover hidden, watched for 320 s, then clicked                                                        | Pass (functional): after 5 min the log shows `popover window released by memory saver` and the "Pulsebar Web Content" process exits; the next click creates a new window and Web Content process and shows it in place. See known issue F1                                  |
| 10  | Monospaced digits in the title           | AX title of the status item and its width every 200 ms, idle and with `yes` × 8                                         | Pass: 39 distinct titles; width constant per string shape (139 pt for `XX%  XXW XX%`, 135 pt for `XX% X.XW XX%`; a one-digit CPU value padded with U+2007 keeps the width). Width changes by 4 pt only when power crosses 10 W (the decimal point is narrower than a digit) |

Observation (not a bug): on the first show of a new window, `CGWindowList` reports it at the creation
position for about 85 ms before the move, but sampled pixels there do not change, so there is no
visible flash.

### Known issues (found in V0, fix planned on a separate `fix/…` branch)

- **F1 (low):** after `window.destroy()` in `popover.rs` (`destroy_if_hidden`, memory saver), the old
  NSWindow stays registered in the WindowServer (still listed by `CGWindowList`, off screen), so one
  NSWindow is kept per memory-saver cycle. Main footprint was 27 MB with the kept window plus the new
  one (`IOAccelerator` 4 MB). Memory saver is off by default.
- **F2 (cosmetic):** on the first render of the settings view in WKWebView, the two cards that hold a
  radio `fieldset` (`SettingRadioGroup`: "Thanh menu", "Giao diện") get about 22–26 px of extra space
  at the bottom (popover 566 px instead of 522 px). Any re-layout, such as toggling a switch, removes
  it. Not reproduced in the Chromium mock.

### Not verified in D2 (chưa kiểm chứng)

- Popover placement on a second display with a different scale factor (only one display available).
- Liquid Glass material (macOS 26+), all Apple Silicon behaviour and budgets.

## D4 — CPU temperature and menu bar metrics (`feat/cpu-temperature`)

Date: 2026-10-15. Release x86_64 bundle built from the D4 working tree
(`pnpm tauri build --target x86_64-apple-darwin --bundles app`), started with `RUST_LOG=pulsebar=debug`,
plus the browser mock (`pnpm dev:mock`) driven by Playwright (Chromium). On-screen checks used the same
CGEvent / `CGWindowList` / `screencapture` helpers as V0 (permissions only read). The owner's
`settings.json` was backed up before the run and restored afterwards (`cmp` identical).

### Automated checks

| AC    | Check                                                                                                                                                                                                                                                                    | Result                                                                                                                                         |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| AC12  | `Snapshot.temperature` and `temperatureUnavailable` serialize to the contract fixture; TS fixture `satisfies` the types; both mock snapshots `satisfies JsonShape<Snapshot>`; settings payload fixture includes `titleMetrics`                                           | Pass (`model.rs`, `settings.rs`, `contract.test.ts`, `contract-settings.test.ts`, `fixtures.test.ts`)                                          |
| AC13  | Intel key probe: `TCXC` preferred, −44 / −127 / 0 / 116 / NaN rejected, fallback order `TCXC → TCXc → TC0D → TC0E → TC0F → TC0P`, no key → missing sensor, 3 bad reads → re-probe, after a failure re-probe only after 60 s; Rosetta and VMs never open the SMC          | Pass (`intel_temp.rs` 14 tests, `power/mod.rs` slot tests)                                                                                     |
| AC14  | Apple Silicon resolver: every row of the decision table (`TCMb` → `TCMz` → `macmon`, firmware key used while the macmon session fails, skipped → keep value, 3 invalid fresh samples → error), 15–130 °C bounds; VM never builds macmon                                  | Pass on x86_64 (14 tests); `cargo clippy` and `cargo test --no-run` for `aarch64-apple-darwin` clean. Runtime on M-series: **chưa kiểm chứng** |
| AC15  | macmon session keeps the sampler for 2 consecutive failures, drops it on the 3rd, resets the counter on success / drop / reopen                                                                                                                                          | Pass (3 new tests, 2 D1 tests updated to 3 failures)                                                                                           |
| AC15b | On a sleep-gap tick the temperature source is read before `reset_sources`                                                                                                                                                                                                | Pass (`temperature_is_read_before_sources_reset_on_a_gap_tick`: `read:3` then `reset`)                                                         |
| AC16  | Default metrics keep the D1 title byte for byte; temperature appended last (`· 55°C` / ` 55°`), unavailable → `–°C` / `–°`, temperature only, empty metrics → `""`; `normalized()` forces the icon; a settings file without `titleMetrics` loads defaults without repair | Pass (6 D1 tray tests and 5 loop tests keep their strings; new tray, loop and settings tests)                                                  |
| AC17  | Vitest: `formatCelsius`, temperature thresholds (80 / 95 °C), temperature history, switch lock rules (all 32 combinations), mock scenarios                                                                                                                               | Pass (Vitest 100 tests)                                                                                                                        |

Gates: `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings` for both targets; `cargo test`
on x86_64 (253 tests); `cargo test --no-run` on aarch64; `pnpm format:check`, `lint`, `check`, `test`,
`build`; no comments in changed files; AC8 greps clean.

### Frontend (mock, Chromium)

| Check                | Method                                                                                                                                                         | Result                                                                                                                                                                                     |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Views render         | Apple Silicon / Intel × light / dark × overview / settings, plus `?temperature=unavailable` and `?rosetta=on` (12 screenshots)                                 | Pass: temperature card right after power (`SMC · TCMb` / `SMC · TCXC`, status label, sparkline); unavailable shows the Rust reason + "Không khả dụng"; Rosetta shows the Rosetta reason    |
| Switch locks         | clicks in the settings view, light and dark                                                                                                                    | Pass: icon off + one metric left → that switch disabled with the hint "Thanh menu cần ít nhất một chỉ số hoặc biểu tượng" (`aria-describedby`); no metric + icon on → icon switch disabled |
| Contrast             | every visible text node of the temperature and "Thanh menu" cards, min over the nominal and worst material backgrounds, × theme × `prefers-contrast` × effects | Pass: text ≥ 5.20:1; temperature chart stroke ≥ 4.78:1                                                                                                                                     |
| Settings card height | section heights in the mock                                                                                                                                    | "Thanh menu" 243 px with the 4 new switches (Giao diện 112 px)                                                                                                                             |

### Real app (release x86_64)

| Check                                  | Method                                                                                    | Result                                                                                                                                                                                                                                                                                              |
| -------------------------------------- | ----------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Key selection                          | start-up log; `cargo run --example probe_intel` (direct SMC reads)                        | Pass: `SMC temperature key TC0D selected`; probe: `TCXC`/`TCXc` absent, `TC0D`/`TC0E`/`TC0F`/`TC0P`/`TC0H` equal (VirtualSMC)                                                                                                                                                                       |
| Temperature card                       | screenshot of the popover window                                                          | Pass: "Nhiệt độ CPU 91 °C · Cao · SMC · TC0D" under background load                                                                                                                                                                                                                                 |
| Default title unchanged                | AX title of the status item with the owner's compact settings                             | Pass: `34%  28W 66%` (D1 format, temperature off)                                                                                                                                                                                                                                                   |
| Temperature on the title               | toggle the switch in the popover; AX title and status item width                          | Compact `24%  24W 67% 81°` (width 139 → 165 pt); default `CPU 20% ·  23W · RAM 68% · 82°C` (216 → 259 pt); switching it off returns the D1 string `CPU 27% ·  25W · RAM 68%`                                                                                                                        |
| Title update latency                   | 11 toggles; click time vs `settings.json` rename vs AX title change (50 ms polling)       | Settings saved 0.26–0.37 s after the click; title changed 0.49–2.25 s after the click (2 of 11 above 2 s). The title is rebuilt on the next sampler tick, so the bound is one 2 s tick plus the settings save; accepted by the owner (same behaviour as the D2 title format switch)                 |
| Temperature rises under load (AC13b)   | `yes > /dev/null` × 8 for 60 s, title every 0.5 s                                         | Pass: 16 samples before the load average 73.8 °C (62–90, background activity); last 44 s of the load average 96.3 °C (peak 100 °C = Tjmax of the i7-7700HQ); rise ≈ 22 °C                                                                                                                           |
| Independent cross-check (AC13b)        | owner ran `sudo powermetrics --samplers smc -i 2000 -n 15` while the app title was logged | Pass on the window mean (criterion accepted by the owner): powermetrics "CPU die temperature" 83.1 °C (74–95) vs app 84.1 °C (76–93), Δ 1.0 °C. Single samples differ by a median of 4 °C because the sensor swings up to ±10 °C between 2 s samples and the two tools sample at different instants |
| "Thanh menu" card height (F2 re-check) | screenshot of the settings view right after opening it                                    | Card ≈ 239 px vs 243 px in Chromium; the 22–26 px extra bottom space of F2 was not seen in this run (F2 stays open until its own fix branch)                                                                                                                                                        |
| Log noise                              | debug log of the whole session                                                            | Pass: no WARN / ERROR                                                                                                                                                                                                                                                                               |

### Performance with the temperature on the title (AC9 / AC18)

Measured with `top -l N -s 10 -pid … -stats pid,command,cpu,mem` (MEM = physical footprint), `footprint`
and `ps -o rss=`, main process plus the app's three WebKit processes. The host was busy (Chrome and
other work running; CPU 20–60 % at "idle").

| Budget                                     | Measured                                                                                                              | Status |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------- | ------ |
| CPU (main process), popover closed ≤ 0.8 % | 0.13 % (first 5 min); 0.16 % over the hour (excluding the 10 min of AX title polling for the powermetrics comparison) | Pass   |
| CPU (main process), popover open ≤ 3 %     | 0.74 % (5 min, visible the whole time); main + WebKit 2.00 %                                                          | Pass   |
| Main footprint ≤ 35 MB                     | 27 MB closed and open (RSS 33–35 MB)                                                                                  | Pass   |
| Total footprint ≤ 120 MB                   | 68–76 MB closed; 91–109 MB open                                                                                       | Pass   |
| Main growth < 5 MB after 1 h (closed)      | footprint 27 → 27 MB (+0 MB); RSS 34.8 → 33.3 MB                                                                      | Pass   |

### Not verified in D4 (chưa kiểm chứng)

- Apple Silicon runtime: `TCMb` / `TCMz` firmware keys, the macmon `cpu_temp_avg` fallback, the 3-failure
  session tolerance on real counters, budgets.
- A real Intel Mac (`TCXC` on genuine hardware); this host's values come from VirtualSMC.
- Rosetta and virtual machines at runtime (covered by unit tests and the mock only).
- VoiceOver reading of the new card and switches (semantics checked in code and the accessibility tree).

## D5 — launch at login through SMAppService (`fix/launch-at-login`)

Date: 2026-10-15/16. Release x86_64 bundles signed ad-hoc with the hardened runtime
(`bundle.macOS.signingIdentity = "-"`, `hardenedRuntime: true`), run from
`src-tauri/target/x86_64-apple-darwin/release/bundle/macos/Pulsebar.app` (not copied to
`/Applications`) with `RUST_LOG=pulsebar=debug`. Toggles were driven through the tray menu and the
popover with the CGEvent helpers; System Settings › General › Login Items was captured with
`screencapture`. The owner approved the first `register()` and every login item created during the
spike was removed afterwards.

### Spike (decision gate)

| Step                                    | Result                                                                                                                                                                                                                                                                           |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1. Enable from the menu                 | Pass: `register()` 69 ms, `status()` Disabled (`SMAppService` 3, NotFound, before the first registration) → Enabled (1); menu shows ✓; no LaunchAgent plist is written                                                                                                           |
| 2. System Settings lists the app        | Pass: "Open at Login" lists "Pulsebar.app"                                                                                                                                                                                                                                       |
| 3. Logout / login starts Pulsebar       | **chưa kiểm chứng** — pending: the owner will log out and back in and report the result                                                                                                                                                                                          |
| 4. Disable                              | Pass: `unregister()` 28 ms, status NotRegistered (0) → Disabled; the entry disappears                                                                                                                                                                                            |
| 5. Rebuild (new cdhash) and read status | Registered with build `d64e20a6…`, rebuilt to `b81ff1cc…`: the new build reads NotFound → Disabled while Login Items still lists the old entry; enabling again adds a second "Pulsebar.app"; disabling removes only the new one. The leftover entry was removed by hand with "−" |

Gate: steps 1, 2 and 4 pass, step 3 pending the owner's report → path 3A (SMAppService, `tauri-plugin-autostart` removed).

`register()` / `unregister()` latency on the main thread over the session: register 69, 110, 66, 36, 4 and 80 ms; unregister 28, 6, 4, 4 and 7 ms (the last pair on the final build). One register call was above the 100 ms threshold of the
plan; the owner chose to keep the calls synchronous (they only run when the setting is toggled).

Ad-hoc signing ties the registration to the code hash, so every rebuild or update of an ad-hoc build
leaves the old entry behind and the new build reports "off". Documented for users in the README (D3):
after updating, remove the old "Pulsebar.app" in Login Items and turn the option on again. A
Developer ID signature should keep the identity across updates (**chưa kiểm chứng**).

### Automated checks

| AC   | Check                                                                                                                                                                                                                                                                                        | Result                                                                                                                                                                             |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| AC19 | `launch_location_issue` (read-only mount, AppTranslocation, `target/debug/pulsebar` → reason; `/Applications/…` and a read-write `/Volumes/Data/…` bundle → none); full `login_action` matrix; status mapping (NotRegistered / NotFound → Disabled); `launch_state` (blocked only while off) | Pass (`login_item.rs` 7 tests)                                                                                                                                                     |
| AC19 | `launchAtLoginStatus` / `launchAtLoginReason` are output-only: forged client values are ignored; contract fixture on both sides (`settings`, `defaultSettings`, `launchAtLoginBlocked`, `launchAtLoginRequiresApproval`)                                                                     | Pass (`settings.rs`, `contract-settings.test.ts`)                                                                                                                                  |
| AC19 | `open_login_items_settings` is in `APP_COMMANDS` and the capability                                                                                                                                                                                                                          | Pass (`ipc::tests::capability_allows_exactly_the_app_commands` checks that the capability allows exactly the `APP_COMMANDS` of `build.rs`; `generate_handler!` checked by reading) |
| UI   | status → switch / hint / button rules for all 4 statuses; store action failure; mock `?login=` scenarios and status recomputation                                                                                                                                                            | Pass (Vitest 123 tests)                                                                                                                                                            |

### Frontend (mock, Chromium)

| Check                     | Result                                                                                                                                                                                                              |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `?login=requiresApproval` | switch on, hint "Cần cho phép Pulsebar trong Cài đặt hệ thống › Mục đăng nhập" linked by `aria-describedby`, button "Mở Cài đặt hệ thống" right after the switch in Tab order; Enter on the button raises no notice |
| `?login=blocked`          | switch natively disabled with the Rust reason as its hint                                                                                                                                                           |
| Contrast                  | light and dark, nominal and worst material: text ≥ 5.20:1 in the "Hệ thống" card                                                                                                                                    |

### Real app (release x86_64, AC20 / AC21)

| Check                                  | Result                                                                                                                                                                           |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Toggle in the popover                  | Pass: on → `Register` → Enabled; off → `Unregister` → Disabled                                                                                                                   |
| Toggle in the menu, menu check in sync | Pass: menu ✓ after enabling from the popover; disabling from the menu updates the switch                                                                                         |
| Theme change while Enabled             | Pass: two `set_settings` calls log `launch-at-login requested true: Noop`, no `register()`                                                                                       |
| Run from a read-only DMG mount         | Pass: status `blocked`; the menu item is disabled; the switch is disabled with "Hãy chép Pulsebar vào thư mục Applications rồi bật lại"; clicking it sends nothing               |
| Signature                              | Pass: `codesign -dv` → `Signature=adhoc`, `flags=0x10002(adhoc,runtime)`; `codesign --verify --deep --strict` valid                                                              |
| Hardened runtime smoke test            | Pass: IOReport power, frequency and `smc:TC0D` temperature are `ok`, the popover renders; main CPU with the popover closed 0.18 % (3 min); main footprint 31 MB; no WARN / ERROR |
| No leftovers                           | Pass: no Pulsebar login item, no `~/Library/LaunchAgents/Pulsebar.plist`; the owner's `settings.json` unchanged (`cmp`)                                                          |

### Not verified in D5 (chưa kiểm chứng)

- Starting at login after logout / login (step 3 of the spike): pending the owner's report.
- `requiresApproval` on a real system (macOS reported Enabled right away here; covered by unit tests
  and the mock only).
- App Translocation at runtime (unit-tested path rule only), Apple Silicon, macOS 15 / 26 / 27,
  Developer ID signed builds.
- Focus when the "Mở Cài đặt hệ thống" button disappears while focused (the popover hides when System
  Settings opens, so focus resets on the next open).
