# Platform matrix

Status of each metric per architecture and macOS version. "chưa kiểm chứng" means not verified on
that platform yet. Updated at the end of each deliverable.

## Data sources

| Metric                         | Apple Silicon (arm64)                                                                                                                                                                                           | Intel (x86_64)                                                                                                   | x86_64 under Rosetta                                      |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| CPU % (total, per logical CPU) | `host_processor_info`                                                                                                                                                                                           | `host_processor_info`                                                                                            | `host_processor_info`                                     |
| Clusters                       | `IODeviceTree:/cpus` + `hw.perflevelN` (E/P/S labels)                                                                                                                                                           | single "CPU" cluster                                                                                             | `hw.perflevelN`                                           |
| CPU power                      | macmon (IOReport energy)                                                                                                                                                                                        | IOReport `pmtelemetry_cpu/msr` package energy → SMC `PCPT` → `PCPC` → `PC0C`                                     | Unavailable ("Đang chạy qua Rosetta – hãy mở bản native") |
| CPU MHz                        | macmon residency-weighted, per core                                                                                                                                                                             | IOReport ACNT/MCNT per logical CPU → nominal + speed limit                                                       | Unavailable                                               |
| CPU temperature                | SMC firmware key `TCMb` (die average) → `TCMz` (max) via `macmon::sources::SMC`, valid 15–130 °C; fallback macmon `cpu_temp_avg` (source `macmon`); VM: Unavailable ("Máy ảo không cung cấp cảm biến nhiệt độ") | SMC key probed by presence `TCXC` → `TCXc` → `TC0D` → `TC0E` → `TC0F` → `TC0P`, valid 10–115 °C; VM: Unavailable | Unavailable ("Đang chạy qua Rosetta – hãy mở bản native") |
| RAM                            | `host_statistics64` + `vm_page_size`                                                                                                                                                                            | same                                                                                                             | same                                                      |
| Network rate                   | `sysctl` `NET_RT_IFLIST2` `if_data64` counters of `en*` interfaces                                                                                                                                              | same                                                                                                             | same                                                      |
| Disks / Wi-Fi IP / uptime      | `getfsstat`, SystemConfiguration + `getifaddrs`, `kern.boottime`                                                                                                                                                | same                                                                                                             | same                                                      |

## Verification status (D1)

| Platform                                | macOS             | CPU %           | RAM             | Power                                  | MHz                                    | Title / tray    |
| --------------------------------------- | ----------------- | --------------- | --------------- | -------------------------------------- | -------------------------------------- | --------------- |
| Intel Hackintosh i7-7700HQ (VirtualSMC) | 14.8.9            | verified        | verified        | verified (IOReport; SMC cross-check)   | probe only                             | verified        |
| Intel Mac (real hardware)               | 14 / 15 / 26      | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng                        | chưa kiểm chứng                        | chưa kiểm chứng |
| Intel Mac with T2                       | 14 / 15 / 26      | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng                        | chưa kiểm chứng                        | chưa kiểm chứng |
| Apple Silicon M1–M5                     | 14 / 15 / 26 / 27 | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng                        | chưa kiểm chứng                        | chưa kiểm chứng |
| Apple Silicon, x86_64 slice via Rosetta | 14 / 15 / 26 / 27 | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng (expected Unavailable) | chưa kiểm chứng (expected Unavailable) | chưa kiểm chứng |
| Virtual machine (Apple Silicon host)    | any               | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng (expected Unavailable) | chưa kiểm chứng (expected Unavailable) | chưa kiểm chứng |

Popover, disks card, Wi-Fi card and performance budgets are covered in the D2 table below.

## Continuous integration

| Runner                          | Architecture | Result                                                                                                                                                                                                                     |
| ------------------------------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| GitHub Actions `macos-26`       | arm64 (VM)   | verified: runs 38560322065 (D1 `3cb01a7`), 38667843038 (D2 `ed7b7d0`), 38723813727 (D4 `bda649a`), 38891681853 (D5 `c727fb0`) and 39003290743 (D3 `db73306`) — fmt, clippy both targets, native Rust tests, frontend gates |
| GitHub Actions `macos-26-intel` | x86_64 (VM)  | verified: same runs                                                                                                                                                                                                        |

CI runners are virtual machines, so they prove the build and unit tests only; Apple Silicon runtime
behaviour (macmon power, topology, budgets) stays **chưa kiểm chứng**.

## Verification status (D2)

| Platform                                | macOS             | Popover show / hide                                                                          | Disks / Wi-Fi / uptime                      | Settings, autostart, single instance | Budgets                                                               |
| --------------------------------------- | ----------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------- | ------------------------------------ | --------------------------------------------------------------------- |
| Intel Hackintosh i7-7700HQ (VirtualSMC) | 14.8.9            | verified: second launch, Reopen, blur, on-screen checks (V0); second display chưa kiểm chứng | verified (data path and card on screen, V0) | verified                             | verified (CPU main process; memory as footprint, see verification.md) |
| Intel Mac (real hardware)               | 14 / 15 / 26      | chưa kiểm chứng                                                                              | chưa kiểm chứng                             | chưa kiểm chứng                      | chưa kiểm chứng                                                       |
| Intel Mac with T2                       | 14 / 15 / 26      | chưa kiểm chứng                                                                              | chưa kiểm chứng                             | chưa kiểm chứng                      | chưa kiểm chứng                                                       |
| Apple Silicon M1–M5                     | 14 / 15 / 26 / 27 | chưa kiểm chứng                                                                              | chưa kiểm chứng                             | chưa kiểm chứng                      | chưa kiểm chứng                                                       |
| Apple Silicon, x86_64 slice via Rosetta | 14 / 15 / 26 / 27 | chưa kiểm chứng                                                                              | chưa kiểm chứng                             | chưa kiểm chứng                      | chưa kiểm chứng                                                       |

## Verification status (D4)

| Platform                                | macOS             | CPU temperature                                                                   | Title metrics (4 switches)                                    |
| --------------------------------------- | ----------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| Intel Hackintosh i7-7700HQ (VirtualSMC) | 14.8.9            | verified: `smc:TC0D`, rises under load, window mean within 1 °C of `powermetrics` | verified (default and compact, on / off, D1 string unchanged) |
| Intel Mac (real hardware)               | 14 / 15 / 26      | chưa kiểm chứng (expected `TCXC` or `TC0D`/`TC0P`)                                | chưa kiểm chứng                                               |
| Intel Mac with T2                       | 14 / 15 / 26      | chưa kiểm chứng                                                                   | chưa kiểm chứng                                               |
| Apple Silicon M1–M5                     | 14 / 15 / 26 / 27 | chưa kiểm chứng (expected `smc:TCMb`, else `macmon`)                              | chưa kiểm chứng                                               |
| Apple Silicon, x86_64 slice via Rosetta | 14 / 15 / 26 / 27 | chưa kiểm chứng (expected Unavailable, unit-tested)                               | chưa kiểm chứng                                               |
| Virtual machine (any host)              | any               | chưa kiểm chứng (expected Unavailable, unit-tested)                               | chưa kiểm chứng                                               |

## Verification status (D5)

| Platform                                | macOS             | Launch at login (SMAppService)                                                                                                                                                                                                  |
| --------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Intel Hackintosh i7-7700HQ (VirtualSMC) | 14.8.9            | verified: register / unregister, Login Items listing, DMG → blocked, ad-hoc + hardened runtime; start at login: Pass (owner report, 2026-10-17; build and location not recorded); enabling from `/Applications` chưa kiểm chứng |
| Intel Mac (real hardware)               | 14 / 15 / 26      | chưa kiểm chứng                                                                                                                                                                                                                 |
| Apple Silicon M1–M5                     | 14 / 15 / 26 / 27 | chưa kiểm chứng                                                                                                                                                                                                                 |

## Verification status (D3)

| Platform                                | macOS             | arm64 bundle                                                                                             | x86_64 bundle                                                                               | Universal bundle                                                                       | DMG install                                                                                                             |
| --------------------------------------- | ----------------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Intel Hackintosh i7-7700HQ (VirtualSMC) | 14.8.9            | built, `lipo`, ad-hoc signature and DMG contents verified; runtime chưa kiểm chứng (cannot run on Intel) | built and verified (`lipo`, signature, DMG contents)                                        | verified: x86_64 slice smoke test (power, frequency, temperature, popover) and budgets | DMG mounts with `Pulsebar.app` and the `Applications` link; copy to `/Applications` and Gatekeeper flow chưa kiểm chứng |
| Intel Mac (real hardware)               | 14 / 15 / 26      | not applicable                                                                                           | chưa kiểm chứng                                                                             | chưa kiểm chứng                                                                        | chưa kiểm chứng                                                                                                         |
| Apple Silicon M1–M5                     | 14 / 15 / 26 / 27 | chưa kiểm chứng                                                                                          | chưa kiểm chứng (runs under Rosetta, expected Unavailable power, frequency and temperature) | chưa kiểm chứng (arm64 slice)                                                          | chưa kiểm chứng                                                                                                         |

Developer ID signing, notarization and stapling: chưa kiểm chứng on every platform (no Developer ID).

## Verification status (network throughput)

| Platform                                | macOS             | Rate (counters, MB/s)                                   | Two-line menu bar image                                           | Popover card    |
| --------------------------------------- | ----------------- | ------------------------------------------------------- | ----------------------------------------------------------------- | --------------- |
| Intel Hackintosh i7-7700HQ (VirtualSMC) | 14.8.9            | verified on `en1` (Wi-Fi); see docs/verification.md     | verified at 1x, Dark appearance; Light appearance chưa kiểm chứng | mock only       |
| Intel Mac (real hardware)               | 14 / 15 / 26      | chưa kiểm chứng                                         | chưa kiểm chứng (Retina 2x)                                       | chưa kiểm chứng |
| Apple Silicon M1–M5                     | 14 / 15 / 26 / 27 | chưa kiểm chứng (compiled, tests built with `--no-run`) | chưa kiểm chứng                                                   | chưa kiểm chứng |

## Verification status (in-app updater)

| Platform                                | macOS             | Check (manual, automatic)                                                                               | Download, signature, install, relaunch                                                                                                | Launch at login after update                                                                                            | `/Applications`, App Management, admin password                                         |
| --------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Intel Hackintosh i7-7700HQ (VirtualSMC) | 14.8.9            | verified against a local endpoint (first automatic check at 60 s); real GitHub endpoint chưa kiểm chứng | verified (x86_64 test builds under `~/Applications`, refusal cases: signature, version mismatch, timeouts, size limit, http URL, DMG) | verified with caveat: registered again, notification shown again, two duplicate entries; starts once after log out / in | `/Applications` and App Management verified (no dialog); admin password chưa kiểm chứng |
| Intel Mac (real hardware)               | 14 / 15 / 26      | chưa kiểm chứng                                                                                         | chưa kiểm chứng                                                                                                                       | chưa kiểm chứng                                                                                                         | chưa kiểm chứng                                                                         |
| Apple Silicon M1–M5                     | 14 / 15 / 26 / 27 | chưa kiểm chứng (compiled, tests built with `--no-run`)                                                 | chưa kiểm chứng                                                                                                                       | chưa kiểm chứng                                                                                                         | chưa kiểm chứng                                                                         |

Details and the refusal cases are in docs/verification.md (D9).

## Releases

| Release | Type             | Assets                                                                          | Built on                                                            |
| ------- | ---------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| v0.1.0  | pre-release      | `aarch64` and `x64` DMGs + `SHA256SUMS`                                         | Intel Hackintosh (dev host), from `f5ccb49`                         |
| v0.2.0  | pre-release      | `aarch64` and `x64` DMGs + `SHA256SUMS`                                         | Intel Hackintosh (dev host), from `3c96798`                         |
| v0.3.0  | release (latest) | `aarch64` and `x64` DMGs, updater packages, `.sig`, `latest.json`, `SHA256SUMS` | Intel Hackintosh (dev host), from the `chore: release 0.3.0` commit |

v0.1.0 and v0.2.0 have no in-app updater. Results per release are in docs/verification.md ("GitHub releases").
