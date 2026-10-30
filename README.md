# Pulsebar

Pulsebar is a macOS menu bar system monitor built with Tauri 2, Rust and Svelte 5. It runs on
macOS 14 or later, on Apple Silicon and Intel Macs, without sudo, kernel extensions or
`powermetrics`, and it starts no child processes.

- **Menu bar title**: CPU usage, CPU power, RAM usage and CPU temperature, each switchable in
  "Thanh menu" (Menu bar), plus an optional icon (at least one metric or the icon stays on).
  "Tốc độ mạng (↑/↓)" (Network speed, off by default) adds the upload and download rate in MB/s as
  two stacked lines between the icon and the title (`↑ 0.3` over `↓ 12.4`, no unit, fixed width). The
  default is CPU, power and RAM with the icon. Two styles: default (`CPU 23% · 8.4W · RAM 61%`) and compact
  (`23% 8.4W 61%`). The digits use a monospaced font and short values are padded with a figure space
  (U+2007), so the width stays stable.
- **Popover** (left click on the title): CPU usage with a 2-minute history and frequency, a CPU
  detail view (per cluster and per core usage and MHz), CPU power, CPU temperature, memory
  breakdown, disks, network ("Mạng": download and upload rate in MB/s with a 2-minute history, Wi-Fi
  interface and IP addresses), uptime, boot time, macOS version and model,
  and the settings.
- **Right-click menu**: "Mở Pulsebar" (Open Pulsebar), "Khởi động cùng macOS" (Launch at login),
  "Cài đặt…" (Settings…), "Thoát" (Quit).

The UI text is Vietnamese. Labels quoted in this README are the exact UI strings, followed by an
English gloss.

Status: version 0.1.0. Builds are signed ad-hoc and not notarized, and no GitHub Release is
published yet, so build from source (see [Build](#build)). Detailed test results live in
[docs/verification.md](docs/verification.md), the per-platform status in
[docs/platform-matrix.md](docs/platform-matrix.md) and the UI contracts in
[docs/ui-inventory.md](docs/ui-inventory.md).

## Contents

- [Support matrix](#support-matrix)
- [Environment setup](#environment-setup)
- [Development](#development)
- [Build](#build)
- [Architecture](#architecture)
- [Data sources](#data-sources)
- [Performance](#performance)
- [Signing and notarization](#signing-and-notarization)
- [Installing and opening an un-notarized build](#installing-and-opening-an-un-notarized-build)
- [Launch at login](#launch-at-login)
- [Logs](#logs)
- [Theme customization](#theme-customization)
- [FAQ](#faq)
- [Known issues](#known-issues)
- [Third-party notices](#third-party-notices)
- [Credits, licences and private APIs](#credits-licences-and-private-apis)

## Support matrix

| Metric                                | Apple Silicon (arm64 build)                                     | Intel (x86_64 build)                                                                        | x86_64 build under Rosetta                                | Virtual machine                                                                                                                                |
| ------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| CPU usage (total and per logical CPU) | yes                                                             | yes                                                                                         | yes                                                       | yes                                                                                                                                            |
| Core clusters                         | E / P / S clusters                                              | one "CPU" cluster                                                                           | clusters from `hw.perflevelN`                             | as the host architecture                                                                                                                       |
| CPU power (W)                         | yes (macmon)                                                    | yes (IOReport package energy, SMC fallback)                                                 | Unavailable ("Đang chạy qua Rosetta – hãy mở bản native") | Apple Silicon VM: Unavailable ("Máy ảo không cung cấp cảm biến công suất"); Intel VM: sensors probed, expected Unavailable (not verified)      |
| CPU frequency (MHz)                   | per core and per cluster (macmon)                               | per logical CPU through IOReport; fallback nominal / max + speed limit without per-core MHz | Unavailable ("Đang chạy qua Rosetta – hãy mở bản native") | Apple Silicon VM: Unavailable ("Máy ảo không cung cấp cảm biến xung nhịp"); Intel VM: expected nominal MHz through the fallback (not verified) |
| CPU temperature                       | SMC firmware key `TCMb` (die average), then `TCMz`, then macmon | first SMC key present among `TCXC`, `TCXc`, `TC0D`, `TC0E`, `TC0F`, `TC0P`                  | Unavailable ("Đang chạy qua Rosetta – hãy mở bản native") | Unavailable ("Máy ảo không cung cấp cảm biến nhiệt độ")                                                                                        |
| Network throughput (MB/s)             | yes                                                             | yes                                                                                         | yes                                                       | counts the guest's own `en*` interfaces (not verified)                                                                                         |
| RAM, disks, Wi-Fi IP, uptime          | yes                                                             | yes                                                                                         | yes                                                       | yes                                                                                                                                            |

An unavailable metric shows "Không khả dụng" (Unavailable) with the reason in the popover, and `–` on
the menu bar title. Use the native build for your Mac: the x86_64 build on Apple Silicon runs under
Rosetta and loses power, frequency and temperature.

Where it was verified:

| Platform                                                              | Status                                                                                                                                                                                                                          |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Intel Hackintosh, Core i7-7700HQ, macOS 14.8.9, VirtualSMC (dev host) | verified: CPU %, RAM, power, temperature (`smc:TC0D`), title, popover, disks, Wi-Fi, uptime, settings, launch-at-login register / unregister; IOReport frequency reports `ok`, values not cross-checked                         |
| Intel Hackintosh, release bundles of D3                               | arm64, x86_64 and universal bundles and DMGs built and checked (`lipo`, ad-hoc signature, DMG contents); the universal bundle's x86_64 slice smoke-tested and measured; the arm64 code was never run (no Apple Silicon machine) |
| GitHub Actions `macos-26` (arm64 VM) and `macos-26-intel` (x86_64 VM) | compile, lint and unit tests only (no bundles)                                                                                                                                                                                  |
| Real Intel Mac (with or without T2)                                   | not verified                                                                                                                                                                                                                    |
| Apple Silicon M1–M5                                                   | not verified                                                                                                                                                                                                                    |
| x86_64 build under Rosetta, virtual machines at runtime               | not verified (unit tests and the browser mock only)                                                                                                                                                                             |
| macOS 15, 26, 27                                                      | not verified                                                                                                                                                                                                                    |

The SMC values on the dev host are emulated by VirtualSMC, so they only indicate how a real Intel
Mac behaves.

## Environment setup

### 1. Xcode Command Line Tools

```bash
xcode-select --install
xcode-select -p
```

### 2. Rust through rustup

The toolchain is pinned by `rust-toolchain.toml` (channel `stable`, components `clippy` and
`rustfmt`, targets `aarch64-apple-darwin` and `x86_64-apple-darwin`); `src-tauri/Cargo.toml`
requires Rust 1.99 or later. Only a rustup-managed `cargo` reads `rust-toolchain.toml`, so install
rustup with one of these options.

Official installer:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Homebrew (the `rustup` formula, not the `rust` formula; it is keg-only and puts its `cargo` and
`rustc` proxies in `$(brew --prefix rustup)/bin`, not in `~/.cargo/bin`):

```bash
brew install rustup
```

Then add this line to `~/.zprofile`, after the `brew shellenv` line, and open a new terminal:

```bash
export PATH="$(brew --prefix rustup)/bin:$PATH"
```

MacPorts (the `rustup` port provides `rustup-init`):

```bash
sudo port install rustup
rustup-init --profile minimal
```

Warning if Rust is already installed from Homebrew (`rust` formula) or MacPorts (`rust` port):
`rustup-init` warns about the existing installation and asks before continuing (with `-y` it only
warns and continues). Both can coexist,
but the rustup proxies must come first in `PATH`. Otherwise the Homebrew or MacPorts `cargo`
shadows rustup, ignores `rust-toolchain.toml` and has no cross-compilation targets. macOS
`path_helper` (in `/etc/zprofile`), Homebrew and MacPorts prepend their directories after
`~/.zshenv` has run, so put this line last in `~/.zprofile` (official installer and MacPorts
variants):

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Install the toolchain, components and both targets:

```bash
rustup default stable
rustup update stable
rustup component add clippy rustfmt
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

### 3. Node.js and pnpm

`package.json` requires Node.js 22.12 or later and pins `pnpm@12.11.2` in `packageManager`; CI uses
Node.js 24. Node.js 25 and later no longer ship Corepack, so Node.js 24 is the simplest choice.

Homebrew (`node@24` is keg-only):

```bash
brew install node@24
```

Then add this line to `~/.zprofile`:

```bash
export PATH="$(brew --prefix node@24)/bin:$PATH"
```

MacPorts:

```bash
sudo port install nodejs24 npm11
```

fnm:

```bash
brew install fnm
fnm install 24
fnm default 24
```

For fnm, also add `eval "$(fnm env --use-on-cd --shell zsh)"` to `~/.zshrc`.

pnpm through Corepack (it downloads the pinned version on first use inside the repository):

```bash
corepack enable pnpm
```

If `corepack` is not found, install it first with `npm install -g corepack` (prefix `sudo` for the
MacPorts Node.js, whose prefix is owned by root).

### 4. Check the environment

Run these inside the repository:

```bash
which -a cargo
rustc -V
rustup show active-toolchain
rustup target list --installed
node -v
pnpm -v
```

Expected: the first `cargo` is a rustup proxy (`~/.cargo/bin/cargo`, or
`$(brew --prefix rustup)/bin/cargo` with the Homebrew formula); `rustc` 1.99 or later; the active
toolchain is `stable`, chosen by `rust-toolchain.toml`; both `aarch64-apple-darwin` and
`x86_64-apple-darwin` are installed; Node.js 22.12 or later; pnpm 12.11.2.

### 5. Install the frontend dependencies

```bash
pnpm install --frozen-lockfile
```

### 6. cargo-about (only to regenerate the notices)

cargo-about 0.9.2 is needed only to regenerate `THIRD-PARTY-NOTICES.txt` with `pnpm notices` (see
[Third-party notices](#third-party-notices)). Building, testing and `pnpm notices:check` do not need
it.

cargo (needs rustup from step 2, Xcode's clang from step 1 and network access to crates.io; it
takes about 3–8 minutes):

```bash
cd ~ && ~/.cargo/bin/cargo install --locked --features cli cargo-about@0.9.2
```

The binary is `~/.cargo/bin/cargo-about`; the install is recorded in `~/.cargo/.crates.toml` and
`~/.cargo/.crates2.json`, and the crate sources add about 112 MB to `~/.cargo/registry`. After a
failed install, remove the leftovers with `rm -rf "$TMPDIR"/cargo-install*`. Uninstall with
`cargo uninstall cargo-about`.

Homebrew (not verified: the dev host has no Homebrew; the binary goes to `/opt/homebrew/bin` on
Apple Silicon and `/usr/local/bin` on Intel):

```bash
brew install cargo-about
```

Check that `brew info cargo-about` reports 0.9.2; otherwise use the cargo command.

MacPorts has no `cargo-about` port; use the cargo command.

Check the install:

```bash
which -a cargo-about
cargo about --version
```

Expected: `cargo-about 0.9.2`.

## Development

Run the app (debug build, Vite dev server on port 1420, hot reload of the popover):

```bash
pnpm tauri dev
```

Run the popover in a browser with mock data (no Rust, no sensors):

```bash
pnpm dev:mock
```

Then open `http://localhost:1420/`. Query parameters select a scenario, for example
`http://localhost:1420/?platform=intel&theme=dark&view=cpu-detail`:

| Parameter     | Values                                         | Effect                                                                 |
| ------------- | ---------------------------------------------- | ---------------------------------------------------------------------- |
| `platform`    | `apple` (default), `intel`                     | Apple M3 Pro (6 E + 6 P, per-core MHz) or Intel i9-9880H (nominal MHz) |
| `theme`       | `light`, `dark`                                | forces the theme; omitted follows the system                           |
| `view`        | `overview` (default), `cpu-detail`, `settings` | initial view                                                           |
| `wifi`        | `off`                                          | Wi-Fi disconnected                                                     |
| `power`       | `unavailable`                                  | power unavailable with the VM reason                                   |
| `temperature` | `unavailable`                                  | temperature unavailable                                                |
| `state`       | `loading`                                      | no data yet (skeletons)                                                |
| `rosetta`     | `on`                                           | Rosetta mode: power, frequency and temperature unavailable             |
| `login`       | `requiresApproval`, `blocked`                  | launch-at-login status scenarios                                       |

The mock module is excluded from production builds. Details: "Mock mode" in
[docs/ui-inventory.md](docs/ui-inventory.md).

### Local gates (same as CI)

Frontend, from the repository root:

```bash
pnpm format:check
pnpm lint
pnpm check
pnpm test
pnpm build
```

Rust, from `src-tauri/` (or add `--manifest-path src-tauri/Cargo.toml` from the root):

```bash
cd src-tauri
cargo fmt --check
cargo clippy --locked --all-targets --target aarch64-apple-darwin -- -D warnings
cargo clippy --locked --all-targets --target x86_64-apple-darwin -- -D warnings
```

Tests run natively for the host architecture and are only compiled for the other one. On an Intel
Mac:

```bash
cargo test --locked --target x86_64-apple-darwin
cargo test --locked --no-run --target aarch64-apple-darwin
```

On Apple Silicon:

```bash
cargo test --locked --target aarch64-apple-darwin
cargo test --locked --no-run --target x86_64-apple-darwin
```

Third-party notices, from the repository root (no cargo-about needed; see
[Third-party notices](#third-party-notices)):

```bash
pnpm notices:check
```

CI (`.github/workflows/ci.yml`) runs all of these on `macos-26` (arm64) and `macos-26-intel`
(x86_64) for every push to `main` and every pull request.

## Build

| Script                 | Command it runs                                                         | Bundles                                             |
| ---------------------- | ----------------------------------------------------------------------- | --------------------------------------------------- |
| `pnpm build:arm64`     | `CI=true tauri build --target aarch64-apple-darwin --bundles app,dmg`   | Apple Silicon `.app` and DMG                        |
| `pnpm build:x64`       | `CI=true tauri build --target x86_64-apple-darwin --bundles app,dmg`    | Intel `.app` and DMG                                |
| `pnpm build:universal` | `CI=true tauri build --target universal-apple-darwin --bundles app,dmg` | universal `.app` and DMG (Tauri runs `lipo` itself) |

`CI=true` makes the DMG bundler skip the Finder AppleScript that lays out the DMG window, so the
build triggers no Automation (TCC) prompt. Each build runs `pnpm build` for the frontend first.

Outputs, with `<triple>` one of `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`universal-apple-darwin`:

| Artifact | Path                                                                                                     |
| -------- | -------------------------------------------------------------------------------------------------------- |
| App      | `src-tauri/target/<triple>/release/bundle/macos/Pulsebar.app`                                            |
| DMG      | `src-tauri/target/<triple>/release/bundle/dmg/Pulsebar_0.1.0_aarch64.dmg` (`_x64.dmg`, `_universal.dmg`) |

The two native DMGs (`aarch64`, `x64`) are the main release artifacts; the universal DMG is
produced as well.

Check the architectures of the main binary (expected `x86_64 arm64` for the universal build):

```bash
lipo -archs src-tauri/target/universal-apple-darwin/release/bundle/macos/Pulsebar.app/Contents/MacOS/pulsebar
```

Check the signature (an ad-hoc build reports `Signature=adhoc` and `flags=0x10002(adhoc,runtime)`;
for the universal app add `--arch x86_64` or `--arch arm64` to check each slice; the DMGs of an
ad-hoc build are not signed):

```bash
codesign --verify --deep --strict --verbose=2 src-tauri/target/universal-apple-darwin/release/bundle/macos/Pulsebar.app
codesign -dv src-tauri/target/universal-apple-darwin/release/bundle/macos/Pulsebar.app
```

Inspect a DMG without opening a Finder window, then compute checksums for publishing:

```bash
mkdir -p "$TMPDIR/pulsebar-dmg"
hdiutil attach -nobrowse -readonly -mountpoint "$TMPDIR/pulsebar-dmg" src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Pulsebar_0.1.0_aarch64.dmg
/bin/ls -la "$TMPDIR/pulsebar-dmg"
hdiutil detach "$TMPDIR/pulsebar-dmg"
shasum -a 256 src-tauri/target/*/release/bundle/dmg/*.dmg
```

Build outputs grow beyond 10 GB in `src-tauri/target/`. Free the space when you no longer need the
bundles (the next build is a full rebuild):

```bash
cargo clean --manifest-path src-tauri/Cargo.toml
```

## Architecture

```text
main thread (AppKit / Tauri event loop)          "sampler" thread (std::thread, every 2 s)
  status item: title, icon, right-click menu  <--  set_title (only when the text changes)
  popover WebviewWindow "popover" (WKWebView) <--  metrics://snapshot (only while visible)
  IPC commands (get_snapshot, set_settings…)  -->  shared state (latest snapshot, flags)
  settings.json, SMAppService                        backends: power, frequency, temperature
```

- **Process**: one Pulsebar process with `ActivationPolicy::Accessory` (no Dock icon), plus the
  WebKit processes of the popover's WKWebView (Web Content, Networking, GPU). Pulsebar starts no
  child processes. `tauri-plugin-single-instance` turns a second launch into "show the popover of
  the running instance".
- **Sampler**: one named `sampler` thread samples every 2 s. A supervisor catches a panic, logs it
  and restarts the loop with a backoff. A sleep/wake gap longer than 3 s skips the power, frequency and
  temperature update of that tick and resets the energy counters (CPU and RAM are still published).
  Every tick also reads the network byte counters (one `sysctl`); a sleep/wake gap restarts the
  rate from a new baseline. Disks and Wi-Fi are rescanned on every tick while the popover is open
  and every fifth tick (10 s) while it is closed; a mounted or unmounted volume emits
  `disks://changed`.
- **Backends**: chosen at compile time by `cfg(target_arch)` (`aarch64` links macmon; `x86_64`
  links the IOReport and SMC readers) and at run time by host facts (`sysctl.proc_translated` for
  Rosetta, `kern.hv_vmm_present` for VMs). Each metric tries a chain of candidates and keeps the
  first that opens; when none works, the metric becomes Unavailable with a Vietnamese reason
  instead of failing.
- **Tray**: an `NSStatusItem` with a monospaced-digit title and an optional template icon. With
  "Tốc độ mạng (↑/↓)" on, the icon slot holds one template image drawn on the main thread
  (`src-tauri/src/tray_image.rs`: `NSBitmapImageRep` at the main screen's backing scale, 9.5 pt
  monospaced digits, logo + two lines), redrawn only when the text changes; the icon setting and
  the rate lines share one state, and the main thread always draws the latest one. Left
  click toggles the popover (a click within 200 ms after the popover lost focus is ignored);
  right click opens the menu.
- **Popover**: a 360 px wide window, pre-created hidden at start-up so the first open is instant,
  placed under the status item with a 12 px rounded native material (or opaque with reduced
  effects). Its height follows the content (200–640 px through `resize_popover`). It hides on blur
  and on Esc. With "Tiết kiệm bộ nhớ" (Memory saver, off by default) a `popover-reaper` thread
  releases the window after 5 minutes hidden.
- **Frontend**: Svelte 5, event driven, no polling and no timers: it fetches the current data when
  the popover is shown and then listens to events. Popover strings live in `src/lib/i18n.ts`; the tray menu labels and the
  Unavailable and launch-at-login reasons are Rust constants. A strict CSP is set in
  `src-tauri/tauri.conf.json`, and `src-tauri/capabilities/default.json` allows the popover window
  exactly the app commands below.
- **Settings**: `~/Library/Application Support/com.thehiep.pulsebar/settings.json`, validated by Rust
  (invalid fields fall back to defaults, files over 64 KiB are rejected) and written atomically
  (temporary file, then rename). The launch-at-login state is read from `SMAppService`, not from
  this file.

IPC surface (names in `src/lib/ipc.ts`, payload types in `src/lib/types.ts`):

| Kind    | Name                        | Purpose                                                                  |
| ------- | --------------------------- | ------------------------------------------------------------------------ |
| command | `get_static_info`           | CPU, topology, model and OS facts (`null` until the sampler has started) |
| command | `get_snapshot`              | latest sample (`null` before the first tick)                             |
| command | `set_detail_enabled`        | fill per-core data while the CPU detail view is open                     |
| command | `get_settings`              | settings plus the launch-at-login status                                 |
| command | `set_settings`              | validate, save and apply settings; returns the stored value              |
| command | `hide_popover`              | hide the popover (Esc)                                                   |
| command | `resize_popover`            | set the popover height (clamped to 200–640)                              |
| command | `quit_app`                  | quit                                                                     |
| command | `open_login_items_settings` | open System Settings › General › Login Items                             |
| event   | `metrics://snapshot`        | a new sample every 2 s, only while the popover is visible                |
| event   | `disks://changed`           | a volume was mounted or unmounted                                        |
| event   | `popover://visibility`      | the popover was shown or hidden (and which view to show)                 |

## Data sources

| Metric          | Apple Silicon                                                                                                                                                    | Intel                                                                                                                                                                                                                                                  |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| CPU usage       | `host_processor_info`                                                                                                                                            | `host_processor_info`                                                                                                                                                                                                                                  |
| Clusters        | `IODeviceTree:/cpus` and `hw.perflevelN` (E / P / S labels)                                                                                                      | single "CPU" cluster                                                                                                                                                                                                                                   |
| CPU power       | macmon 0.9.0 (IOReport energy)                                                                                                                                   | IOReport `pmtelemetry_cpu/msr` package energy, then SMC `PCPT`, then `PCPC`, then `PC0C`                                                                                                                                                               |
| CPU frequency   | macmon, residency-weighted MHz per core                                                                                                                          | IOReport `MSR_IA32_ACNT` / `MSR_IA32_MCNT` per logical CPU (base MHz × ΔACNT / ΔMCNT); if IOReport is missing, nominal and max MHz (`hw.cpufrequency`, `hw.cpufrequency_max`) plus the speed limit from `IOPMCopyCPUPowerStatus`, without per-core MHz |
| CPU temperature | SMC firmware key `TCMb` (die average), then `TCMz` (maximum), read through `macmon::sources::SMC`, valid 15–130 °C; fallback macmon `cpu_temp_avg`               | first SMC key present among `TCXC`, `TCXc`, `TC0D`, `TC0E`, `TC0F`, `TC0P`, valid 10–115 °C                                                                                                                                                            |
| RAM             | `host_statistics64` and `vm_page_size`                                                                                                                           | same                                                                                                                                                                                                                                                   |
| Disks           | `getfsstat`                                                                                                                                                      | same                                                                                                                                                                                                                                                   |
| Network rate    | `sysctl` `NET_RT_IFLIST2` (64-bit `if_data64` byte counters) of the `en*` interfaces that are up and running, summed; rate = byte delta / time between two ticks | same                                                                                                                                                                                                                                                   |
| Wi-Fi IP        | SystemConfiguration and `getifaddrs`                                                                                                                             | same                                                                                                                                                                                                                                                   |
| Uptime          | `kern.boottime`                                                                                                                                                  | same                                                                                                                                                                                                                                                   |

Fallback rules:

- Power shows Unavailable when the counters stop updating: 0 W for 3 or more samples while CPU usage
  is above 5 % ("Bộ đếm năng lượng không cập nhật", the energy counter is not updating).
- Intel temperature: readings outside 10–115 °C (for example negative values or 0) and NaN are
  rejected; three bad reads trigger a new key probe, and after a failure the probe is retried only
  after 60 s.
- Apple Silicon: the macmon session survives two consecutive failures, is dropped on the third and
  is reopened later.
- Rosetta and virtual machines never open the SMC or macmon for temperature.
- Network rate: only `en0`, `en1`, … are counted (Wi-Fi, Ethernet, USB / Thunderbolt Ethernet,
  iPhone tethering). Loopback, VPN tunnels (`utun*`, `ipsec*`), AirDrop (`awdl*`, `llw*`), bridges and
  virtual machine interfaces are skipped, so traffic is never counted twice; Bluetooth PAN and
  `pdp_ip*` modems are not counted. It reads local kernel counters and generates no traffic. The
  first tick, a tick after sleep and a tick shorter than 0.5 s show no value; an interface whose
  counter goes backwards is skipped for that tick. MB/s is decimal (bytes/s ÷ 1,000,000).

## Performance

Budgets (CPU of the main process; memory as physical footprint, the "Memory" column of Activity
Monitor):

| Budget                               | Intel    | Apple Silicon (original spec target, not measured)                 |
| ------------------------------------ | -------- | ------------------------------------------------------------------ |
| CPU, popover closed                  | ≤ 0.8 %  | ≤ 0.5 %                                                            |
| CPU, popover open                    | ≤ 3 %    | ≤ 2 %                                                              |
| Main process footprint               | ≤ 35 MB  | not set yet                                                        |
| Total (main + WebKit)                | ≤ 120 MB | footprint budget not set yet (the spec's ≤ 100 MB is an RSS value) |
| Main footprint growth after 1 h idle | < 5 MB   | < 5 MB                                                             |

The memory budgets use footprint instead of RSS (owner decision, 2026-10-14): an RSS of 25 MB or
less is not reachable for a WKWebView host with a menu bar status item. RSS is reported only as
extra information in [docs/verification.md](docs/verification.md).

Measured on the dev host (Intel Hackintosh i7-7700HQ, VirtualSMC, a busy machine), so the numbers
are indicative only:

| Build                                                 | CPU, popover closed                                                              | CPU, popover open (main / main + WebKit) | Main footprint           | Total footprint                 |
| ----------------------------------------------------- | -------------------------------------------------------------------------------- | ---------------------------------------- | ------------------------ | ------------------------------- |
| x86_64 release, D4 (temperature on the title)         | 0.13 % (first 5 min), 0.16 % (1 h)                                               | 0.74 % / 2.00 % (5 min)                  | 27 MB, flat over 1 h     | 68–76 MB closed, 91–109 MB open |
| x86_64 release, ad-hoc + hardened runtime, D5 (smoke) | 0.18 % (3 min)                                                                   | not measured                             | 31 MB                    | not measured                    |
| Universal build (x86_64 slice), D3                    | 0.13 % (5 min; one show / hide not triggered by the test fell inside the window) | 0.68 % / 1.89 % (3 min)                  | 26 MB closed, 27 MB open | 67–73 MB closed, 85 MB open     |

Method: find the main process and the app's three WebKit processes, then sample them with `top`
(its `mem` column is the physical footprint) and `footprint`:

```bash
pgrep -x pulsebar
lsappinfo list
top -l 30 -s 10 -pid <main-pid> -pid <webkit-pid> -stats pid,command,cpu,mem
footprint <pid>
```

Measure the closed state with the popover hidden, and the open state only while it is visible.
Never rebuild a bundle while it is being measured.

## Signing and notarization

By default the bundles are signed ad-hoc with the hardened runtime (`bundle.macOS.signingIdentity`
`"-"` and `hardenedRuntime: true` in `src-tauri/tauri.conf.json`) and are not notarized. An ad-hoc
build with the hardened runtime and no entitlements still reads IOReport and AppleSMC (measured on
the dev host). Developer ID signing and notarization are not verified: no Developer ID was
available.

Never commit certificates, passwords or API keys, and keep them out of shell history; the values
below are placeholders.

Sign with a Developer ID identity from your keychain (`APPLE_SIGNING_IDENTITY` overrides the `"-"`
of `signingIdentity`):

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
pnpm build:arm64
```

Or import the certificate from a `.p12` file (useful in CI). Set `APPLE_SIGNING_IDENTITY` as well:
Tauri infers the identity from the certificate only when neither the environment nor
`tauri.conf.json` sets one, and this repository sets `"-"`. Read the password without echoing it so
it stays out of the shell history:

```bash
export APPLE_CERTIFICATE="$(openssl base64 -A -in cert.p12)"
read -rs APPLE_CERTIFICATE_PASSWORD
export APPLE_CERTIFICATE_PASSWORD
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
```

Tauri notarizes the `.app` during the build when one of these sets is present. Apple ID with an
app-specific password, read without echoing it (Tauri passes the value to `notarytool --password`
as is, so the `@keychain:` prefix of the old `altool` does not apply):

```bash
export APPLE_ID="you@example.com"
read -rs APPLE_PASSWORD
export APPLE_PASSWORD
export APPLE_TEAM_ID="TEAMID"
```

Or an App Store Connect API key:

```bash
export APPLE_API_ISSUER="<issuer-id>"
export APPLE_API_KEY="<key-id>"
export APPLE_API_KEY_PATH="$HOME/.private_keys/AuthKey_<key-id>.p8"
```

Tauri notarizes the app but not the DMG (observed bundler behaviour; not verified with real
credentials): with a Developer ID identity it is expected to only sign the DMG, and with the default
ad-hoc identity the DMG is not signed at all (measured: `codesign -dv` reports
`code object is not signed at all` for all three DMGs, while the app inside each DMG passes
`codesign --verify --deep --strict`). Notarize and staple the DMG by hand. Store the credentials once
in the keychain (`notarytool` prompts for the app-specific password), then submit:

```bash
xcrun notarytool store-credentials pulsebar-notary --apple-id you@example.com --team-id TEAMID
DMG=src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Pulsebar_0.1.0_aarch64.dmg
xcrun notarytool submit "$DMG" --keychain-profile pulsebar-notary --wait && xcrun stapler staple "$DMG"
```

With an API key, replace `--keychain-profile pulsebar-notary` by
`--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER"`. Repeat for the
x64 and universal DMGs.

Check the result:

```bash
codesign --verify --deep --strict --verbose=2 src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Pulsebar.app
codesign -dv src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Pulsebar.app
spctl -a -vv -t exec src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Pulsebar.app
spctl -a -vv -t open --context context:primary-signature src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Pulsebar_0.1.0_aarch64.dmg
```

`spctl` rejects an ad-hoc build (measured: `rejected` for the arm64, x86_64 and universal apps); that
is expected until the build is signed with Developer ID and notarized.

## Installing and opening an un-notarized build

1. Pick the DMG for your Mac: `Pulsebar_0.1.0_aarch64.dmg` for Apple Silicon,
   `Pulsebar_0.1.0_x64.dmg` for Intel, or `Pulsebar_0.1.0_universal.dmg` for either. Do not use the
   x64 DMG on Apple Silicon (Rosetta disables power, frequency and temperature).
2. Open the DMG and copy `Pulsebar.app` to `/Applications`. Run it from there, not from the DMG
   (launch at login only works from a writable location).
3. Open it once despite the missing notarization. This applies to a downloaded build, which macOS
   marks with the quarantine attribute; a build you made yourself is not quarantined. Not verified
   on the dev host (no quarantined download was tested):
   - macOS 15 or later: open the app, dismiss the warning, then go to System Settings › Privacy &
     Security and click "Open Anyway".
   - macOS 14: Control-click the app in Finder, choose Open, then confirm.
   - Or remove the quarantine attribute from a terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Pulsebar.app
```

Pulsebar has no Dock icon; it appears in the menu bar.

## Launch at login

"Khởi động cùng macOS" (Launch at login) is off by default. Turn it on in the popover settings
("Hệ thống", System) or in the right-click menu. Pulsebar registers itself with
`SMAppService.mainApp` (objc2-service-management 0.3.2); it writes no LaunchAgent. The entry
appears as "Pulsebar.app" in System Settings › General › Login Items.

The option can only be turned on when Pulsebar runs from a writable location outside the DMG and
outside App Translocation, so copy it to `/Applications` first. From a read-only DMG mount the
switch is disabled with the hint "Hãy chép Pulsebar vào thư mục Applications rồi bật lại" (Copy
Pulsebar to the Applications folder, then turn it on again). A `pnpm tauri dev` build shows "Bản
chạy thử không đăng ký được" (A development build cannot be registered).

| Status             | What the settings show                                                                                                                                                                       |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `enabled`          | switch on                                                                                                                                                                                    |
| `disabled`         | switch off                                                                                                                                                                                   |
| `requiresApproval` | switch on, hint "Cần cho phép Pulsebar trong Cài đặt hệ thống › Mục đăng nhập" (Allow Pulsebar in System Settings › Login Items) and the button "Mở Cài đặt hệ thống" (Open System Settings) |
| `blocked`          | switch disabled, with the reason as its hint                                                                                                                                                 |

Ad-hoc signing caveat (measured): an ad-hoc signature ties the registration to the code hash. After
you update or rebuild an ad-hoc build, the old "Pulsebar.app" entry stays in Login Items and the
new build reports the option as off. To fix it, open System Settings › General › Login Items,
select the old "Pulsebar.app", click "−", then turn the option on again in Pulsebar. A Developer ID
signature should keep the identity across updates (not verified).

Development builds made before the switch to `SMAppService` (they used `tauri-plugin-autostart`)
may have written `~/Library/LaunchAgents/Pulsebar.plist` (label `Pulsebar`, pointing to the old
binary). Remove it by hand:

```bash
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/Pulsebar.plist 2>/dev/null; rm -f ~/Library/LaunchAgents/Pulsebar.plist
```

The start after logging out and back in passed by owner report (2026-10-17; build and location not
recorded). Not verified yet: turning the option on from a copy in `/Applications`,
`requiresApproval` on a real system, App Translocation at run time (only the path rule is
unit-tested; the DMG block was checked on the dev host), Apple Silicon, macOS 15 / 26 / 27,
Developer ID builds.

## Logs

Pulsebar logs through `env_logger` to standard error. The default level is `warn`, and messages are
written only when a state changes (a backend comes up or goes down, a metric becomes Unavailable,
the sampler restarts), never on every tick at the default level:

| Level   | Examples                                                                 |
| ------- | ------------------------------------------------------------------------ |
| `error` | the sampler thread crashed (restarted with a backoff) or could not start |
| `warn`  | a metric or backend is unavailable, invalid settings were replaced       |
| `info`  | backend selection, a metric is available again, sleep/wake gaps          |
| `debug` | popover shown or hidden, `SMAppService` call durations                   |
| `trace` | every menu bar title change                                              |

To see the logs of an installed build, quit the running instance first (right-click › "Thoát",
Quit), because a second launch hands over to the running instance and exits. Then run:

```bash
RUST_LOG=pulsebar=debug /Applications/Pulsebar.app/Contents/MacOS/pulsebar
```

During development:

```bash
RUST_LOG=pulsebar=debug pnpm tauri dev
```

## Theme customization

The popover styles use design tokens only:

- `src/styles/tokens.css`: non-colour tokens (font family and sizes, spacing, radii, control sizes,
  durations, easing), the `prefers-reduced-motion: reduce` override (durations become `0ms`) and
  the `prefers-contrast: more` override of the disabled opacity.
- `src/styles/themes.css`: colour tokens per theme in `:root, :root[data-theme='light']` and
  `:root[data-theme='dark']`, the `@media (prefers-contrast: more)` overrides, and
  `:root[data-effects='reduced']`, declared last so it wins (opaque window and cards).

Rule: colour literals (`#…`, `rgb(…)`, `rgba(…)`, `hsl(…)`) may appear only in these two files;
every component uses `var(--…)`. The production CSP forbids inline styles, so put new values in
these files rather than in markup.

In the app, "Giao diện" (Appearance) has "Chủ đề" (Theme) with "Theo hệ thống" (System), "Sáng"
(Light) and "Tối" (Dark), and "Giảm hiệu ứng trong suốt" (Reduce transparency effects), which makes
the popover opaque. The popover also follows the macOS "Reduce transparency", "Reduce motion" and
"Increase contrast" settings.

After changing colours, check every text pair at 4.5:1 or more and chart strokes, borders and
progress fills at 3:1 or more, in light and dark, with and without increased contrast. The current
tokens pass with the worst-case material background; the method and the ratio tables are in
[docs/ui-inventory.md](docs/ui-inventory.md). Preview both themes with `pnpm dev:mock` and
`?theme=light` or `?theme=dark`.

## FAQ

**Why does the title show `–W`?** Power is Unavailable: the app runs under Rosetta, in a virtual
machine, on a model without a power sensor, or the energy counters stopped updating (0 W for 3 or
more samples while CPU usage is above 5 %). The popover shows the reason.

**How is MHz measured on Intel?** Per logical CPU through IOReport (`MSR_IA32_ACNT` /
`MSR_IA32_MCNT`) when it is available. Otherwise Pulsebar shows the nominal and maximum frequency
with the speed limit, and the CPU detail view says "Không hỗ trợ MHz từng lõi trên Mac này"
(Per-core MHz is not supported on this Mac). Intel Power Gadget is discontinued and not used.

**Why is there no Wi-Fi network name (SSID)?** By design: Pulsebar never reads the SSID. On recent
macOS versions reading it requires Location Services permission. The Wi-Fi card shows the interface
and the IPv4 / IPv6 addresses.

**Is the MHz on an M4 the real clock?** On M4 chips the reported value is the requested DVFS level,
not the measured clock (macmon issue #78).

**Why does the temperature differ from Stats or iStat Menus?** On Apple Silicon the tools read
different sensors: Pulsebar uses the `TCMb` die average, while Stats averages other keys. On Intel,
Pulsebar probes keys in the same order as Stats. Pulsebar was not compared with Stats or iStat Menus
directly. The sensor also swings by up to ±10 °C between 2 s samples, and each tool
samples at a different instant. On the dev host, the 30 s window mean matched `powermetrics` within
1 °C.

**Can it show Fahrenheit?** No. Temperatures are shown in °C only; there is no unit setting.

## Known issues

Both were found during on-screen checks and are planned for a separate fix branch.

- **F1 (low)**: with "Tiết kiệm bộ nhớ" (Memory saver) on, the released popover window stays
  registered in the WindowServer, so one window is kept per memory-saver cycle (main footprint
  27 MB with the kept window plus the new one). Memory saver is off by default.
- **F2 (cosmetic)**: on the first render of the settings view, the "Thanh menu" (Menu bar) and
  "Giao diện" (Appearance) cards can get about 22–26 px of extra space at the bottom. Any re-layout,
  such as toggling a switch, removes it. It was not seen again in the D4 run, but it stays open.

## Third-party notices

`bundle.resources` in `src-tauri/tauri.conf.json` copies `LICENSE` and `THIRD-PARTY-NOTICES.txt`
from the repository root into every bundle, as `Pulsebar.app/Contents/Resources/LICENSE` and
`Pulsebar.app/Contents/Resources/THIRD-PARTY-NOTICES.txt`. tauri-build checks at compile time that
both files exist, so `THIRD-PARTY-NOTICES.txt` is committed: without it every `cargo build`,
`cargo clippy` and `cargo test` fails.

| Command              | What it does                                           | Needs                                                                                                                                                                   |
| -------------------- | ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm notices`       | regenerates `THIRD-PARTY-NOTICES.txt`                  | cargo-about 0.9.2 ([step 6](#6-cargo-about-only-to-regenerate-the-notices)) and network access to crates.io and to the GitHub raw files of the pinned `clarify` entries |
| `pnpm notices:check` | checks the committed file; the last step of the CI job | no cargo-about                                                                                                                                                          |

What the file lists:

- Rust crates, generated by cargo-about 0.9.2 with `src-tauri/about.toml` and `src-tauri/about.hbs`
  for both `aarch64-apple-darwin` and `x86_64-apple-darwin`: every crate linked into the app binary
  on either target, plus proc-macro and build-host crates (a harmless superset).
- JavaScript packages, from Vite's `build.license` output for the bundled frontend, plus a static
  entry for Vite's own modulepreload polyfill (Vite does not list virtual modules).

Regenerate the file with `pnpm notices` and commit it whenever `src-tauri/Cargo.lock`,
`pnpm-lock.yaml` or the frontend imports change. `pnpm notices:check` fails when:

- a crate linked into the app (per `cargo tree -e normal,no-proc-macro`) is missing from the file;
- a listed crate version is no longer in the lockfile dependency closure (stale after an update);
- the npm package set differs from the Vite build;
- a crate in the closure ships a `NOTICE` file, whose content must then be added to the notices by
  hand.

Licence texts:

- `about.toml` puts Apache-2.0 before MIT in `accepted`, so a crate licensed "MIT OR Apache-2.0" is
  listed with the Apache-2.0 text; recipients may choose either licence.
- Crates that ship no licence file, or not the file their SPDX expression needs, are pinned with
  `clarify` entries that carry the SHA-256 checksum of the upstream file. When such a crate is
  updated, the checksum no longer matches and `cargo about generate --fail` stops; recompute the
  entry with `cargo about clarify`.
- The released `objc2`, `block2`, `objc2-encode` and `objc2-foundation` crates are MIT-licensed but
  ship only an upstream `LICENSE.md` statement without the MIT terms. Upstream added the MIT text to
  its repository later (commit `ee9a7ada2131f5944b8750428e265c15632f2a19`); a byte-for-byte copy is
  kept in `src-tauri/licenses/objc2-LICENSE-MIT.txt` and appended to the Rust section as a
  supplementary text. The scripts check its SHA-256, and `pnpm notices` stops if one of those crates
  leaves the dependency tree.

## Credits, licences and private APIs

- Apple Silicon power, frequency and SMC access use [macmon](https://github.com/vladkens/macmon)
  0.9.0 by vladkens, under the MIT licence.
- Built with [Tauri](https://tauri.app), [Svelte](https://svelte.dev) and
  [Lucide](https://lucide.dev) icons.
- Pulsebar is released under the MIT License ([`LICENSE`](LICENSE), matching `license = "MIT"` in
  `src-tauri/Cargo.toml`). The licences of the third-party code it ships are in
  `THIRD-PARTY-NOTICES.txt` (see [Third-party notices](#third-party-notices)).

Private API warning: IOReport, AppleSMC, `IODeviceTree` and Tauri's `macOSPrivateApi` (used for the
transparent popover window) are private or undocumented, and a macOS update can break them. Every
read path falls back to Unavailable with a reason instead of crashing. For the same reason Pulsebar
is not suitable for the Mac App Store and does not use the App Sandbox.
