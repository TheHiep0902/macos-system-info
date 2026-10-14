# Platform matrix

Status of each metric per architecture and macOS version. "chưa kiểm chứng" means not verified on
that platform yet. Updated at the end of each deliverable.

## Data sources

| Metric                         | Apple Silicon (arm64)                                            | Intel (x86_64)                                                               | x86_64 under Rosetta                                      |
| ------------------------------ | ---------------------------------------------------------------- | ---------------------------------------------------------------------------- | --------------------------------------------------------- |
| CPU % (total, per logical CPU) | `host_processor_info`                                            | `host_processor_info`                                                        | `host_processor_info`                                     |
| Clusters                       | `IODeviceTree:/cpus` + `hw.perflevelN` (E/P/S labels)            | single "CPU" cluster                                                         | `hw.perflevelN`                                           |
| CPU power                      | macmon (IOReport energy)                                         | IOReport `pmtelemetry_cpu/msr` package energy → SMC `PCPT` → `PCPC` → `PC0C` | Unavailable ("Đang chạy qua Rosetta – hãy mở bản native") |
| CPU MHz                        | macmon residency-weighted, per core                              | IOReport ACNT/MCNT per logical CPU → nominal + speed limit                   | Unavailable                                               |
| RAM                            | `host_statistics64` + `vm_page_size`                             | same                                                                         | same                                                      |
| Disks / Wi-Fi IP / uptime      | `getfsstat`, SystemConfiguration + `getifaddrs`, `kern.boottime` | same                                                                         | same                                                      |

## Verification status (D1)

| Platform                                | macOS             | CPU %           | RAM             | Power                                  | MHz                                    | Title / tray    |
| --------------------------------------- | ----------------- | --------------- | --------------- | -------------------------------------- | -------------------------------------- | --------------- |
| Intel Hackintosh i7-7700HQ (VirtualSMC) | 14.8.9            | verified        | verified        | verified (IOReport; SMC cross-check)   | probe only                             | verified        |
| Intel Mac (real hardware)               | 14 / 15 / 26      | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng                        | chưa kiểm chứng                        | chưa kiểm chứng |
| Intel Mac with T2                       | 14 / 15 / 26      | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng                        | chưa kiểm chứng                        | chưa kiểm chứng |
| Apple Silicon M1–M5                     | 14 / 15 / 26 / 27 | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng                        | chưa kiểm chứng                        | chưa kiểm chứng |
| Apple Silicon, x86_64 slice via Rosetta | 14 / 15 / 26 / 27 | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng (expected Unavailable) | chưa kiểm chứng (expected Unavailable) | chưa kiểm chứng |
| Virtual machine (Apple Silicon host)    | any               | chưa kiểm chứng | chưa kiểm chứng | chưa kiểm chứng (expected Unavailable) | chưa kiểm chứng (expected Unavailable) | chưa kiểm chứng |

Popover, disks card, Wi-Fi card and performance budgets are verified in D2.
