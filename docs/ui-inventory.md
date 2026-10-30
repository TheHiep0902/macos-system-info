# UI inventory

Component contracts for the Pulsebar popover. Source files carry no comments (project rule), so
each component's props, data, states and tokens are documented here. The TypeScript `interface Props`
in each component is the authoritative prop contract; this file explains intent.

## IPC surface used by the frontend

Names live in `src/lib/ipc.ts`; payload types in `src/lib/types.ts`.

| Kind    | Name                        | Payload / arguments              | Notes                                                                                                         |
| ------- | --------------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| command | `get_static_info`           | → `StaticInfo \| null`           | `null` until the sampler has started; re-invoked on every popover show                                        |
| command | `get_snapshot`              | → `Snapshot \| null`             | latest sample; `null` before the first tick (render skeletons)                                                |
| command | `set_detail_enabled`        | `{ enabled: boolean }`           | `true` while the CPU detail view is open, so `cpu.cores[]` is filled                                          |
| command | `get_settings`              | → `Settings`                     | the three `launchAtLogin*` fields come from `SMAppService`, not the settings file                             |
| command | `set_settings`              | `{ settings: Settings }` → saved | Rust validates, persists atomically and applies; returns the stored value and the real launch-at-login status |
| command | `hide_popover`              | —                                | used for Esc                                                                                                  |
| command | `resize_popover`            | `{ height: number }`             | logical px; Rust clamps to 200–640                                                                            |
| command | `quit_app`                  | —                                |                                                                                                               |
| command | `open_login_items_settings` | —                                | opens System Settings › General › Login Items; returns nothing                                                |
| event   | `metrics://snapshot`        | `Snapshot`                       | every 2 s, only while the popover is visible                                                                  |
| event   | `disks://changed`           | `null`                           | a volume was mounted or unmounted                                                                             |
| event   | `popover://visibility`      | `PopoverVisibility`              | emitted by Rust on show/hide; `view` requests a view on show                                                  |

Payload fields added in D4:

- `Snapshot.temperature: MetricState<TemperatureSample>` (after `power`), with
  `TemperatureSample { celsius: number; source: string }`. `source` is `smc:<KEY>` (for example
  `smc:TC0D` on Intel, `smc:TCMb` on Apple Silicon) or `macmon` (Apple Silicon fallback).
  `unavailable` carries a Vietnamese reason from Rust (no sensor, VM, Rosetta).
- `Settings.titleMetrics: TitleMetrics` with `cpu`, `power`, `memory`, `temperature` booleans:
  which values the menu bar title shows. Default `cpu`, `power`, `memory` on and `temperature` off
  (the D1 title). Rust `normalized()` forces `showTrayIcon = true` when no title metric is on and the
  icon is off; the settings view never sends that state (see `SettingsView` below).

Payload fields added for the network rate (`feat/network-throughput`):

- `Snapshot.throughput: MetricState<ThroughputSample>` (after `network`), with
  `ThroughputSample { rxBytesPerSec: number; txBytesPerSec: number; interfaces: string[] }` (bytes per
  second summed over the counted `en*` interfaces). `loading` on the first tick and after a sleep
  gap; `unavailable` carries "Không đọc được lưu lượng mạng" when the kernel counters cannot be read.
- `Settings.titleMetrics.network: boolean`, default `false`; a settings file without the key reads
  as `false` without repair. It controls the two-line rate image on the menu bar, not the title text.

Payload fields added in D5 (launch at login through `SMAppService`):

- `Settings.launchAtLoginStatus: LaunchAtLoginStatus`, one of `enabled`, `disabled`,
  `requiresApproval` (registered, but the user has not allowed it in System Settings yet) or `blocked`
  (login is off and Pulsebar runs from a read-only mount such as a DMG, from App Translocation or
  outside an `.app` bundle, so registering would not survive).
- `Settings.launchAtLoginReason: string | null`: a Vietnamese reason from Rust, set only when the
  status is `blocked` (for example "Hãy chép Pulsebar vào thư mục Applications rồi bật lại"). The
  frontend displays it as-is.
- `Settings.launchAtLogin: boolean` stays and equals `enabled || requiresApproval`; it is the only
  launch field the frontend asks Rust to change.
- Both new fields are output-only. The store still sends the whole `Settings` object to
  `set_settings`, including whatever status and reason it last received, and Rust ignores them on
  input; the payload Rust returns replaces them.

## Source layout

| Path                         | Role                                                                                               |
| ---------------------------- | -------------------------------------------------------------------------------------------------- |
| `src/main.ts`                | sets the initial `data-theme`, creates the transport and store, mounts `App`, calls `init()`       |
| `src/App.svelte`             | view switch (`overview` / `cpu-detail` / `settings`), root attributes, Esc, focus restoration      |
| `src/lib/transport.ts`       | `Transport` interface, the Tauri implementation, mock selection via dynamic import                 |
| `src/lib/store.svelte.ts`    | `PopoverStore`: event-driven state, history buffers, settings saves, command failures              |
| `src/lib/format.ts`          | vi-VN number, byte, MHz, W, °C, uptime and date formatting; source, scope and cluster labels       |
| `src/lib/thresholds.ts`      | warn / critical thresholds and their text labels                                                   |
| `src/lib/title-metrics.ts`   | `TITLE_METRIC_KEYS` and `menuBarLocks`, the pure lock rule for the menu bar switches               |
| `src/lib/launch-at-login.ts` | `launchAtLoginUi`, the pure status → switch state, hint and button rule for "Khởi động cùng macOS" |
| `src/lib/i18n.ts`            | every Vietnamese string, typed keys and typed `{placeholder}` interpolation                        |
| `src/lib/theme.ts`           | `resolveTheme`, `resolveEffects` and the media queries they follow                                 |
| `src/lib/sparkline.ts`       | polyline points and min / max for the sparkline text equivalent                                    |
| `src/lib/motion.ts`          | `fadeToken` transition whose duration is read from `--duration-fast`                               |
| `src/styles/tokens.css`      | non-colour tokens and the reduced-motion override                                                  |
| `src/styles/themes.css`      | colour tokens per theme, increased-contrast overrides, reduced-effects overrides                   |
| `src/styles/base.css`        | reset, focus ring, `.sr-only`, `.selectable`, shared `.metric-*` classes, motion pausing           |
| `src/mocks/transport.ts`     | mock backend for `pnpm dev:mock` (reachable only through the dynamic import in `transport.ts`)     |
| `src/mocks/*.json`           | mock fixtures (Apple Silicon and Intel `StaticInfo` / `Snapshot`) and the IPC contract fixtures    |

Colour literals (`#…`, `rgb(…)`, `rgba(…)`, `hsl(…)`) exist only in `tokens.css` and `themes.css`;
every other colour, spacing, radius, size, font, duration and easing is a `var(--…)`.

## Tokens

### Non-colour tokens (`src/styles/tokens.css`)

| Token                                                         | Value                                                              |
| ------------------------------------------------------------- | ------------------------------------------------------------------ |
| `--font-family-sans`                                          | `-apple-system, BlinkMacSystemFont, system-ui, sans-serif`         |
| `--font-size-xs` / `-sm` / `-md` / `-lg` / `-xl` / `-display` | 11 / 12 / 13 / 15 / 20 / 24 px                                     |
| `--font-weight-regular` / `-medium` / `-semibold`             | 400 / 500 / 600                                                    |
| `--line-height-tight` / `-normal`                             | 1.2 / 1.4                                                          |
| `--space-1` … `--space-8`                                     | 2, 4, 6, 8, 10, 12, 16, 20 px                                      |
| `--radius-sm` / `-md` / `-lg` / `-pill`                       | 4 / 8 / 12 / 999 px                                                |
| `--size-hairline`, `--border-width`                           | 1 px                                                               |
| `--focus-ring-width` / `--focus-ring-offset`                  | 2 px / 2 px                                                        |
| `--icon-size-sm` / `-md`                                      | 14 / 16 px                                                         |
| `--control-size`                                              | 28 px (icon buttons, quit button min height)                       |
| `--progress-height` / `-sm`                                   | 6 / 4 px                                                           |
| `--sparkline-height` / `--sparkline-stroke`                   | 28 px / 1.5 px                                                     |
| `--status-dot-size`                                           | 8 px                                                               |
| `--switch-width` / `--switch-height` / `--switch-knob-size`   | 30 / 18 / 14 px                                                    |
| `--radio-size` / `--radio-dot-size`                           | 14 / 6 px                                                          |
| `--core-cell-min-width`                                       | 84 px                                                              |
| `--skeleton-line-height`                                      | 10 px                                                              |
| `--opacity-disabled`                                          | 0.4; 0.6 under `prefers-contrast: more` (disabled switch control)  |
| `--duration-fast` / `--duration-base`                         | 120 ms / 200 ms; both `0ms` under `prefers-reduced-motion: reduce` |
| `--easing-standard`                                           | `cubic-bezier(0.2, 0, 0, 1)`                                       |

### Colour tokens (`src/styles/themes.css`)

`:root` and `[data-theme='light']` carry the light values; `[data-theme='dark']` overrides them. The
"more" columns are the `@media (prefers-contrast: more)` overrides.

| Token                         | Light                    | Dark                     | Light, more contrast     | Dark, more contrast      |
| ----------------------------- | ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `--color-bg-root`             | `transparent`            | `transparent`            | —                        | —                        |
| `--color-bg-solid`            | `#ececef`                | `#1f1f22`                | —                        | —                        |
| `--color-bg-window`           | `rgba(246,246,248,0.7)`  | `rgba(30,30,33,0.7)`     | `rgba(246,246,248,0.88)` | `rgba(30,30,33,0.88)`    |
| `--color-surface-card`        | `rgba(255,255,255,0.72)` | `rgba(255,255,255,0.06)` | `rgba(255,255,255,0.92)` | —                        |
| `--color-surface-card-solid`  | `#ffffff`                | `#2c2c30`                | —                        | —                        |
| `--color-surface-hover`       | `rgba(0,0,0,0.05)`       | `rgba(255,255,255,0.07)` | —                        | —                        |
| `--color-surface-active`      | `rgba(0,0,0,0.09)`       | `rgba(255,255,255,0.12)` | —                        | —                        |
| `--color-surface-track`       | `rgba(0,0,0,0.1)`        | `rgba(255,255,255,0.14)` | `rgba(0,0,0,0.16)`       | `rgba(255,255,255,0.12)` |
| `--color-surface-skeleton`    | `rgba(0,0,0,0.08)`       | `rgba(255,255,255,0.1)`  | —                        | —                        |
| `--color-surface-badge`       | `rgba(0,0,0,0.06)`       | `rgba(255,255,255,0.1)`  | `rgba(0,0,0,0.08)`       | `rgba(255,255,255,0.12)` |
| `--color-surface-accent`      | `rgba(0,80,184,0.1)`     | `rgba(120,184,255,0.12)` | `rgba(0,65,154,0.12)`    | `rgba(166,207,255,0.14)` |
| `--color-surface-warn`        | `rgba(127,76,0,0.12)`    | `rgba(255,190,92,0.12)`  | `rgba(101,59,0,0.14)`    | `rgba(255,211,147,0.14)` |
| `--color-surface-critical`    | `rgba(176,31,23,0.1)`    | `rgba(255,143,133,0.12)` | `rgba(146,24,16,0.12)`   | `rgba(255,177,170,0.14)` |
| `--color-text-primary`        | `#1d1d1f`                | `#f5f5f7`                | `#000000`                | `#ffffff`                |
| `--color-text-secondary`      | `#46464c`                | `#d2d2d7`                | `#2a2a2e`                | `#ececf0`                |
| `--color-text-tertiary`       | `#55555c`                | `#b4b4bb`                | `#36363c`                | `#d8d8de`                |
| `--color-text-badge-accent`   | `#0050b8`                | `#8cc2ff`                | `#00419a`                | `#a6cfff`                |
| `--color-text-badge-warn`     | `#7f4c00`                | `#ffc46b`                | `#653b00`                | `#ffd393`                |
| `--color-text-badge-critical` | `#9c1912`                | `#ffa69e`                | `#921810`                | `#ffb1aa`                |
| `--color-text-on-control`     | `#ffffff`                | `#1f1f22`                | —                        | —                        |
| `--color-border-subtle`       | `rgba(0,0,0,0.08)`       | `rgba(255,255,255,0.1)`  | `rgba(0,0,0,0.3)`        | `rgba(255,255,255,0.35)` |
| `--color-border-strong`       | `#8a8a90`                | `#8e8e95`                | `#55555c`                | `#b4b4bb`                |
| `--color-accent`              | `#0050b8`                | `#78b8ff`                | `#00419a`                | `#a6cfff`                |
| `--color-status-normal`       | `#156a2c`                | `#62d684`                | `#0e5522`                | `#8ce8a6`                |
| `--color-status-warn`         | `#7f4c00`                | `#ffbe5c`                | `#653b00`                | `#ffd393`                |
| `--color-status-critical`     | `#b01f17`                | `#ff8f85`                | `#921810`                | `#ffb1aa`                |
| `--color-status-unavailable`  | `#55555c`                | `#b4b4bb`                | `#36363c`                | `#d8d8de`                |
| `--color-chart-cpu`           | `#0a64d6`                | `#5eaaff`                | `#0050b8`                | `#8cc2ff`                |
| `--color-chart-power`         | `#c25a00`                | `#ffa552`                | `#9a4700`                | `#ffbe85`                |
| `--color-chart-memory`        | `#7b3fc4`                | `#c09cff`                | `#6230a8`                | `#d2b8ff`                |
| `--color-chart-disk`          | `#08786f`                | `#4dd4c6`                | `#06615a`                | `#7fe3d8`                |
| `--color-chart-network`       | `#2e7d32`                | `#7bd88f`                | `#236326`                | `#9fe6ad`                |
| `--color-chart-temperature`   | `#c0267a`                | `#ff85c0`                | `#931656`                | `#ffb0d4`                |

`[data-effects='reduced']` (declared last, so it wins over theme and contrast blocks) sets
`--color-bg-window` to `--color-bg-solid` and `--color-surface-card` to
`--color-surface-card-solid`, giving an opaque, 12 px rounded shell and opaque cards;
`--color-bg-root` stays `transparent` so the window corners outside the shell remain clear.

### Contrast ratios

WCAG 2.x relative-luminance ratios, computed with a script over every token pair the UI renders.
Translucent tokens are alpha-composited before measuring. Three backgrounds are checked per theme:

- **solid**: the reduced-effects path (`--color-bg-solid` window, `--color-surface-card-solid` card);
- **nominal**: `--color-bg-window` over a typical native material (light `#e9e9ec`, dark `#28282b`),
  then `--color-surface-card` over that;
- **worst**: the same chain over a pessimistic material (light material over a very dark wallpaper
  `#9a9aa0`, dark material over a very light wallpaper `#5e5e64`).

The table lists the minimum of the three. Text needs ≥ 4.5:1; UI graphics (chart strokes, borders,
progress fills against the track) need ≥ 3:1.

| Pair                                            | Light              | Dark               | Light, more        | Dark, more         |
| ----------------------------------------------- | ------------------ | ------------------ | ------------------ | ------------------ |
| text-primary on card / window                   | 15.41 / 12.11      | 9.85 / 11.86       | 20.72 / 17.64      | 12.64 / 15.15      |
| text-secondary on card / window                 | 8.58 / 6.74        | 7.12 / 8.57        | 14.10 / 12.01      | 10.73 / 12.86      |
| text-tertiary on card / window                  | 6.77 / 5.32        | 5.20 / 6.27        | 11.84 / 10.08      | 8.91 / 10.67       |
| accent text on card / window                    | 6.77 / 5.32        | 5.16 / 6.21        | 9.32 / 7.94        | 7.82 / 9.37        |
| status-normal text on card / window             | 6.14 / 4.83        | 5.86 / 7.06        | 8.85 / 7.53        | 8.55 / 10.25       |
| status-warn text on card / window               | 6.55 / 5.15        | 6.52 / 7.86        | 9.51 / 8.09        | 9.03 / 10.81       |
| status-critical text on card / window           | 6.30 / 4.95        | 4.86 / 5.86        | 8.78 / 7.47        | 7.30 / 8.74        |
| status-unavailable text on card / win           | 6.77 / 5.32        | 5.20 / 6.27        | 11.84 / 10.08      | 8.91 / 10.67       |
| badge neutral text on badge bg                  | 7.52               | 5.26               | 11.79              | 7.35               |
| badge accent text on badge bg                   | 5.79               | 4.64               | 7.57               | 5.64               |
| badge warn text on badge bg                     | 5.48               | 5.28               | 7.50               | 6.32               |
| badge critical text on badge bg                 | 6.36               | 4.68               | 7.06               | 5.36               |
| notice (critical) on window                     | 5.05               | 5.55               | 6.06               | 6.38               |
| chart-cpu / power / memory on card              | 5.05 / 4.04 / 5.74 | 4.43 / 5.49 / 4.82 | 7.30 / 6.33 / 8.26 | 6.79 / 7.80 / 7.25 |
| chart-disk / network on card                    | 4.90 / 4.70        | 5.90 / 6.16        | 7.23 / 7.18        | 8.36 / 8.67        |
| chart-temperature on card                       | 5.06               | 4.78               | 8.32               | 7.47               |
| border-strong on card                           | 3.14               | 3.30               | 7.29               | 6.13               |
| progress fill normal / warn / critical on track | 4.91 / 5.23 / 5.03 | 3.83 / 4.27 / 3.18 | 6.10 / 6.56 / 6.05 | 5.86 / 6.18 / 5.00 |
| accent fill (switch on) on track                | 5.41               | 3.38               | 6.43               | 5.36               |

`--color-chart-temperature` is a rose / magenta, kept apart from `--color-status-critical` (red),
`--color-chart-power` (orange) and `--color-chart-memory` (purple); its ratios were computed with the
same three-background method and clear the 3:1 chart minimum by a wide margin in every theme.

On the solid reduced-effects background alone, the lowest text ratios are 5.83 (light,
status-critical on window) and 6.30 (dark, status-critical on card). Native vibrancy can only be
approximated: the "worst" column is the guard rail, not a measurement of a real desktop.

## Components

Every component takes plain data (never the store) except `App.svelte`. Icons are imported one by one
(`@lucide/svelte/icons/<name>`) and sized through tokens in CSS, never with props.

### layout

| Component      | Props                                                                                   | Data source                               | States                                                                            | Tokens                                                                                                                                                                         |
| -------------- | --------------------------------------------------------------------------------------- | ----------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `PopoverShell` | `header?`, `footer?`, `children` (snippets), `notice`, `busy`, `onheightchange(height)` | parent view                               | notice banner (`role="alert"`), `aria-busy` on `<main>`                           | `--color-bg-window`, `--radius-lg` (matches the native 12 px material radius), `--space-*`, `--color-border-subtle`, `--color-surface-critical`, `--color-text-badge-critical` |
| `Header`       | `staticInfo`, `uptimeSecs`                                                              | `store.staticInfo`, `snapshot.uptimeSecs` | no static info → app name + "Đang khởi động bộ đo…"; Rosetta badge + warning line | `--font-size-lg`, `--color-status-warn`, `--color-text-secondary`                                                                                                              |
| `Footer`       | `onsettings`, `onquit`, `settingsRef` (bindable)                                        | —                                         | —                                                                                 | `--color-text-tertiary`, `--space-*`                                                                                                                                           |
| `ViewHeader`   | `title`, `onback`                                                                       | —                                         | focuses the back button on mount                                                  | `--font-size-lg`, `--space-3`                                                                                                                                                  |

`PopoverShell` is a column: fixed `<header>`, scrolling `<main>`, fixed `<footer>`. A
`ResizeObserver` watches the header, the unconstrained content wrapper inside `<main>` and the footer,
and reports their summed height. `App` forwards it to `store.resize`, which rounds up, clamps to
200–640 and invokes `resize_popover` only when the integer height changes. If the content is taller
than 640 px the window stays at 640 and `<main>` scrolls.

### cards

| Component         | Props                                                                 | Data source                                                          | States                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | Tokens                                                          |
| ----------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| `MetricCard`      | `title`, `icon`, `tone`, `busy`, `aside?`, `children`                 | parent card                                                          | `aria-busy`; `<section aria-labelledby>` with an `<h2>`; `tone` is `cpu`, `power`, `temperature`, `memory`, `disk`, `network` or `uptime`                                                                                                                                                                                                                                                                                                                                                                            | `--color-surface-card`, `--radius-lg`, `--color-chart-*` (icon) |
| `CpuCard`         | `snapshot`, `staticInfo`, `history`, `onopen`, `buttonRef` (bindable) | `snapshot.cpu`, `store.history.cpu`                                  | skeleton; frequency loading / unavailable (reason) / `ioReport` average / `nominal` base MHz; cluster MHz when more than one cluster reports it; status label                                                                                                                                                                                                                                                                                                                                                        | `--color-accent`, `--color-chart-cpu`, status colours           |
| `PowerCard`       | `power`, `history`                                                    | `snapshot.power`, `store.history.power`                              | skeleton (null or `loading`); unavailable → Rust reason + "Không khả dụng"; ok → smoothed W, scope ("CPU" / "Gói CPU"), `SourceBadge`                                                                                                                                                                                                                                                                                                                                                                                | `--color-chart-power`                                           |
| `TemperatureCard` | `temperature`, `history`                                              | `snapshot.temperature`, `store.history.temperature`                  | placed right after `PowerCard`; skeleton "Đang đo nhiệt độ…" (null or `loading`); unavailable → Rust reason + "Không khả dụng"; ok → whole °C (`formatCelsius`), `StatusLabel` from `statusFor('temperature', celsius)` (warn 80 °C, critical 95 °C), `SourceBadge` ("SMC · {key}" or "macmon"), sparkline on a 0–100 °C scale (raised to the highest value in the history when a reading exceeds 100 °C) + `.sr-only` min / max sentence                                                                            | `--color-chart-temperature`, status colours                     |
| `MemoryCard`      | `memory`                                                              | `snapshot.memory`                                                    | skeleton; % + status label, used / total, breakdown (app, wired, compressed, cached, swap)                                                                                                                                                                                                                                                                                                                                                                                                                           | `--color-chart-memory`, status colours                          |
| `DiskCard`        | `disks`, `changing`                                                   | `snapshot.disks`, `store.disksChanging`                              | skeleton; `EmptyState` for an empty list; boot volume first; badges Khởi động / Ổ rời / Ổ mạng; status label when ≥ warn; "Khả dụng …" + "Khả dụng gồm dung lượng purgeable" when `availableImportantBytes` is set, else "Còn trống …"; mount / unmount fade                                                                                                                                                                                                                                                         | `--color-chart-disk`, `--duration-fast` (via `fadeToken`)       |
| `NetworkCard`     | `network`, `throughput`, `rxHistory`, `txHistory`                     | `snapshot.network`, `snapshot.throughput`, `store.history.rx` / `tx` | title "Mạng"; rate grid always shown: "Tải xuống" / "Tải lên" labels in `<dt>`, arrows `aria-hidden`, `formatRate` values (MB/s, vi-VN decimals), sparklines from 0 to the series maximum + `.sr-only` min / max sentence, no `aria-live`; loading → skeleton; unavailable → reason + "Không khả dụng"; then the Wi-Fi block: skeleton; connected → interface, IPv4, IPv6 (selectable text); "Chưa có địa chỉ IP"; disconnected → `EmptyState` "Không kết nối Wi-Fi" + primary interface if any. SSID is never shown | `--color-chart-network`                                         |
| `UptimeCard`      | `uptimeSecs`, `staticInfo`                                            | `snapshot.uptimeSecs`, `staticInfo`                                  | skeleton; uptime, boot time, macOS version, model                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | `--font-size-lg`                                                |

### cpu

| Component        | Props                                                          | Data source                                    | States                                                                                                                                                                                                                                                       | Tokens                                    |
| ---------------- | -------------------------------------------------------------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| `CpuDetailView`  | `snapshot`, `staticInfo`, `notice`, `onheightchange`, `onback` | store                                          | skeleton until both are present; "{physical} lõi vật lý / {logical} luồng"; topology-assumed note; Apple Silicon → `ClusterSummary`, Intel → `FrequencyPanel`; "Không hỗ trợ MHz từng lõi trên Mac này" when frequency is ok but `perCoreAvailable` is false | `--color-border-subtle`, `--font-size-md` |
| `ClusterSummary` | `clusters`, `info`, `frequency`                                | `snapshot.cpu.clusters`, `staticInfo.clusters` | per cluster: Vietnamese name, usage bar, cluster MHz, max MHz from `ClusterInfo`; reason when frequency is unavailable; "Đang đo xung nhịp…" while loading                                                                                                   | status colours                            |
| `FrequencyPanel` | `frequency`, `staticInfo`                                      | `snapshot.cpu.frequency`, `staticInfo`         | loading → skeleton; unavailable → reason; `ioReport` → average / min / max (+ speed limit); `nominal` → "Cơ bản … · Tối đa … · Giới hạn tốc độ …"                                                                                                            | `--font-size-sm`                          |
| `CoreGrid`       | `cores`, `clusters`, `showMhz`, `groupByCluster`, `labelKind`  | `snapshot.cpu.cores`                           | empty `cores[]` (detail not yet sampled) → skeleton; grouped by cluster in `ClusterInfo` order with `<h3>`; Intel shows one ungrouped grid of logical CPUs                                                                                                   | `--core-cell-min-width`, `--space-3`      |
| `CoreCell`       | `core`, `label`, `showMhz`                                     | one `CoreSample`                               | usage %, small bar (`aria-valuetext` includes the status label), MHz when `showMhz`                                                                                                                                                                          | `--color-surface-card`, `--font-size-xs`  |

No Hyper-Threading pairing is assumed: Intel cells are labelled "CPU {id}" (logical CPU index), Apple
Silicon cells "Lõi {id}".

### primitives

| Component     | Props                                                      | Notes                                                                                                                                                                                                                             | Tokens                                                                                                    |
| ------------- | ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `ProgressBar` | `value` (0–100), `label`, `valueText?`, `status?`, `size?` | `role="progressbar"`, `aria-valuenow/min/max`, `aria-label`, `aria-valuetext`; fill width is a `scaleX` set through `style:transform`                                                                                             | `--progress-height(-sm)`, `--color-surface-track`, status colours, `--duration-fast`, `--easing-standard` |
| `Sparkline`   | `values`, `tone`, `capacity?` (60), `max?`                 | plain SVG `<polyline>`, right-aligned, `aria-hidden`; callers render a `.sr-only` min / max sentence next to it; `tone` is `cpu`, `power`, `temperature` or `memory`                                                              | `--sparkline-height`, `--sparkline-stroke`, `--color-chart-*`                                             |
| `StatusDot`   | `status`                                                   | decorative (`aria-hidden`); always paired with text                                                                                                                                                                               | `--status-dot-size`, status colours                                                                       |
| `StatusLabel` | `status`                                                   | dot + "Bình thường" / "Cao" / "Rất cao" / "Không khả dụng"                                                                                                                                                                        | status colours                                                                                            |
| `Badge`       | `tone?`, `children`                                        | neutral / accent / warn / critical pill                                                                                                                                                                                           | `--color-surface-badge/-accent/-warn/-critical`, `--color-text-badge-*`                                   |
| `EmptyState`  | `title`, `description?`, `icon?`, `tone?`                  | empty lists, no Wi-Fi, settings load error                                                                                                                                                                                        | `--color-text-secondary/-tertiary`, `--color-status-critical`                                             |
| `Skeleton`    | `lines?`, `label?`                                         | static placeholder bars (no shimmer animation) + `.sr-only` label                                                                                                                                                                 | `--color-surface-skeleton`, `--skeleton-line-height`                                                      |
| `IconButton`  | `label`, `icon`, `onclick`, `disabled?`, `ref` (bindable)  | native `<button>`, `aria-label` and `title` from `label`                                                                                                                                                                          | `--control-size`, `--icon-size-md`, `--color-surface-hover/-active`                                       |
| `SourceBadge` | `source`                                                   | `ioreport`/`ioReport` → "IOReport", `smc:<KEY>` → "SMC · <KEY>" with the key's case kept (`smc:PCPT` → "SMC · PCPT", `smc:TCMb` → "SMC · TCMb"), `nominal` → "Danh định", `macmon` → "macmon"; `.sr-only` "Nguồn số liệu:" prefix | `--color-border-subtle`, `--color-text-tertiary`                                                          |

### settings

| Component           | Props                                                                                                                                   | Notes                                                                                                                                                                                                                                                                                                                     |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SettingsView`      | `settings`, `saving`, `saveFailed`, `loadFailed`, `notice`, `onheightchange`, `onchange(patch)`, `onback`, `onquit`, `onopenloginitems` | sections Thanh menu / Giao diện / Ổ đĩa / Hệ thống; skeleton while `settings` is null, `EmptyState` if `get_settings` failed; "Đang lưu…" in a `role="status"` line; inline `role="alert"` error after a failed save; launch-at-login hint and "Mở Cài đặt hệ thống" button (see below); quit button                      |
| `SettingSwitch`     | `label`, `hint?`, `checked`, `disabled?`, `onchange(checked)`                                                                           | native `<input type="checkbox" role="switch" aria-checked>` with a `<label for>`; hint linked by `aria-describedby`; Enter toggles too; `disabled` sets the native attribute (announced as dimmed, skipped by Tab, no handler runs), dims the control with `--opacity-disabled` and the label to `--color-text-secondary` |
| `SettingRadioGroup` | `legend`, `name`, `value`, `options`, `onchange(value)` (generic over the option type)                                                  | `<fieldset>` + `<legend>` + native radios (arrow keys move the selection)                                                                                                                                                                                                                                                 |

The "Thanh menu" section holds, in order: the "Kiểu tiêu đề" radio group; a `role="group"` labelled
"Chỉ số trên thanh menu" (a plain `<div>` with `aria-labelledby`, not a `<fieldset>`, so the known
first-render spacing issue of `SettingRadioGroup` cannot spread to it) with four indented switches
"Mức dùng CPU", "Công suất CPU", "Mức dùng RAM", "Nhiệt độ CPU" bound to `settings.titleMetrics`; and
the "Hiện biểu tượng" switch. A metric switch sends a patch with the whole `titleMetrics` object
(current values plus the toggled key), so the store still saves the whole `Settings` object.

Lock rule (`menuBarLocks` in `src/lib/title-metrics.ts`, unit-tested over all 32 combinations): the
menu bar must always show at least one title metric or the icon.

- Exactly one metric on and the icon off → that metric's switch is `disabled`.
- No metric on and the icon on → the icon switch is `disabled`.
- Otherwise nothing is disabled. The state "no metric and icon off" never comes from Rust
  (`normalized()`); if it ever did, nothing is locked so the user can turn the icon back on.

At most one switch is locked at a time, and from any state Rust can send a switch never locks itself
as a result of its own toggle, so focus is never lost. The locked switch shows the hint "Thanh menu
cần ít nhất một chỉ số hoặc biểu tượng" under its label through the existing `hint` prop, which
links it with `aria-describedby`.

The "Hệ thống" section holds "Tiết kiệm bộ nhớ" and "Khởi động cùng macOS". The launch switch is
bound to `settings.launchAtLogin` (`checked`) and sends `{ launchAtLogin }`; everything else about it
comes from `launchAtLoginUi(settings)` in `src/lib/launch-at-login.ts` (unit-tested for every status),
which reads only `launchAtLoginStatus` and `launchAtLoginReason`:

| Status             | Switch                                  | Hint (`hint` prop, linked by `aria-describedby`)                                                               | "Mở Cài đặt hệ thống" button |
| ------------------ | --------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ---------------------------- |
| `enabled`          | on, enabled                             | —                                                                                                              | —                            |
| `disabled`         | off, enabled                            | —                                                                                                              | —                            |
| `requiresApproval` | on (Rust sends `launchAtLogin: true`)   | "Cần cho phép Pulsebar trong Cài đặt hệ thống › Mục đăng nhập"                                                 | shown under the switch       |
| `blocked`          | off, native `disabled` (skipped by Tab) | `launchAtLoginReason` from Rust as-is; when it is null or blank, "Không bật được khởi động cùng macOS lúc này" | —                            |

While a save is in flight the switch follows the optimistic `launchAtLogin`, but the status (and so
the hint and button) keeps the last value Rust sent; the store never guesses a status. A reason sent
with any status other than `blocked` is ignored.

The "Mở Cài đặt hệ thống" button is a native `<button type="button">` whose visible text is its
accessible name, with a decorative `external-link` icon (`aria-hidden`). It sits after the switch in
DOM and Tab order, is `align-self: flex-start`, and reuses the quit button's look: `--control-size`
min height, `--space-2` / `--space-6` padding, `--color-border-subtle` border, `--radius-md`,
`--color-surface-card` background with `--color-surface-hover` / `--color-surface-active` states,
`--color-text-primary` text at `--font-size-md`, `--icon-size-sm` icon and the global
`:focus-visible` ring. It calls `onopenloginitems`, which `App` maps to
`store.openLoginItemsSettings()`; a failure shows the shell's action notice ("Không thực hiện được
thao tác. Hãy thử lại."). After the user allows Pulsebar in System Settings, the next popover show
refreshes settings and the status becomes `enabled`, which removes the hint and the button.

No new colour pairs: the hint is `--color-text-tertiary` on the card (≥ 5.20:1 in every theme, see
the contrast table) and the button text is `--color-text-primary` on the card (≥ 9.85:1).

## State, theming and accessibility

### Store lifecycle (`PopoverStore`)

1. `main.ts` builds the transport (`createTransport()`), constructs the store with the initial view
   (`overview`, or the mock's `?view=`), mounts `App`, then awaits `store.init()`.
2. `init()` registers the three listeners first (`metrics://snapshot`, `disks://changed`,
   `popover://visibility`), then runs `refresh()` (`get_settings`, `get_static_info`, `get_snapshot` in
   parallel). It never waits for a visibility event, because the window can be shown before the
   listeners exist. If it starts in `cpu-detail` it sends `set_detail_enabled({ enabled: true })`.
3. `popover://visibility` `visible: true` → `visible = true`, switch to `view` when it is not null,
   re-send `set_detail_enabled(true)` when the current view is `cpu-detail` (Rust resets it on hide),
   and `refresh()`. A `null` `StaticInfo` keeps the previous value. `visible: false` only sets
   `visible = false`; nothing is invoked.
4. Every `metrics://snapshot` sets `visible = true` (self-heal after a missed visibility event) and is
   applied only if it is newer (`seq` or `timestampMs` greater) than the current snapshot.
5. History: ring buffers of the last 60 values for CPU %, RAM %, smoothed W, °C and the network
   download / upload rate in MB/s (`rx`, `tx`). A buffer restarts
   when `seq` does not advance by exactly 1 or the timestamp gap is negative or above 10 s. The power
   buffer is emptied while power is not `ok`, the temperature buffer while temperature is not
   `ok`, and the `rx` / `tx` buffers while throughput is not `ok`.
6. `disks://changed` sets `disksChanging` until the next applied snapshot; `DiskCard` exposes it as
   `aria-busy` plus a polite status line, and new / removed rows fade with `--duration-fast`.
7. `setView('cpu-detail')` sends `set_detail_enabled(true)`; leaving it sends `false`.
8. `updateSettings(patch)` applies the patch locally at once, then queues `set_settings({ settings })`
   with the full object (including the whole `titleMetrics`); saves run one at a time. Success
   replaces local settings with the value Rust returns; failure restores the last confirmed value and
   sets `settingsError`. The patch never touches `launchAtLoginStatus` / `launchAtLoginReason`:
   toggling launch at login changes only `launchAtLogin` locally, and the returned payload brings the
   real status and reason.
9. Actions: `hide()` (`hide_popover`), `quit()` (`quit_app`) and `openLoginItemsSettings()`
   (`open_login_items_settings`, no arguments) each invoke one command and resolve even on failure.
10. Every invoke is wrapped: failures never reject, they are logged once per failure streak with
    `console.warn('[pulsebar] <command> failed: <message>')` (message truncated to 200 chars, no
    arguments logged) and recorded in `failures`. `dataFailed` (snapshot / static info / listener
    failure) and `actionFailed` (hide / quit / detail / open Login Items) drive the shell's notice
    banner; settings failures show inline in the settings view.
11. No timers anywhere in the store; `dispose()` removes all listeners (used by tests).

### Theme, effects and visibility

- `<html data-theme>`: `settings.theme` when it is `light` or `dark`; for `system` (and before
  settings load) it follows `matchMedia('(prefers-color-scheme: dark)')` with a `change` listener.
  `main.ts` sets it once synchronously before mounting to avoid a light flash.
- `<html data-effects="reduced">` when `settings.reduceEffects` is on or
  `(prefers-reduced-transparency: reduce)` matches (also observed with a `change` listener); the shell
  becomes opaque while `html` stays transparent, so the 12 px shell radius still shapes the window
  (Rust also removes the native material). Otherwise `html` is transparent so the native vibrancy
  shows through the translucent window and card tokens.
- `<html data-visible="false">` while the popover is hidden pauses CSS animations and disables
  transitions (`base.css`).

### Keyboard map

| Key                                   | Effect                                                                                                           |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Tab / Shift+Tab                       | moves through "Chi tiết", footer settings / quit, back buttons, settings controls (a disabled switch is skipped) |
| Enter / Space                         | activates buttons; Space toggles switches and Enter toggles them too                                             |
| Arrow keys                            | change the selected radio inside a radio group                                                                   |
| ↑ / ↓, PgUp / PgDn, Home / End, Space | scroll the content area when focus is not in a form control (Space only when focus is not on a button)           |
| Esc                                   | `hide_popover` from any view                                                                                     |

Focus: entering CPU detail or settings focuses the back button; returning to the overview focuses
the control that opened the view ("Chi tiết" or the settings button). The focus ring is
`--focus-ring-width` solid `--color-accent`, offset `--focus-ring-offset`, on `:focus-visible`.
The "Chi tiết" button stretches over the CPU card with a `::after` overlay, so clicking anywhere on the
card opens the detail view while the accessible control stays a real `<button>`.

### ARIA and semantics

- One `<h1>` per view (CPU brand, "Chi tiết CPU" or "Cài đặt"), `<h2>` per card or section, `<h3>`
  per core cluster group.
- Cards are `<section aria-labelledby>`; the shell uses `<header>`, `<main>` and `<footer>`.
- `ProgressBar` is `role="progressbar"` with `aria-valuenow/min/max`, `aria-label` and a formatted
  `aria-valuetext`.
- Sparklines are `aria-hidden`; a `.sr-only` sentence states the 2-minute min and max.
- Status is never colour alone: every warn / critical colour is paired with "Cao" / "Rất cao" text
  (or included in `aria-valuetext` for core cells).
- `IconButton` always has an `aria-label`; icons are `aria-hidden`.
- The menu bar metric switches sit in a `role="group"` named "Chỉ số trên thanh menu"; a locked switch
  uses the native `disabled` attribute and is described by the visible hint.
- The launch-at-login switch follows the same pattern: `blocked` sets native `disabled` and the Rust
  reason is its visible, `aria-describedby`-linked hint; `requiresApproval` keeps it operable, links
  the approval hint and adds the "Mở Cài đặt hệ thống" button, named by its visible text.
- Live regions: notice banner and settings save error are `role="alert"`; settings saving and disk
  updates are `role="status"`.

### Reduced motion and increased contrast

- `prefers-reduced-motion: reduce` sets `--duration-fast` and `--duration-base` to `0ms` and
  `base.css` removes all transitions and animations. The disk fade reads `--duration-fast`, so it
  becomes instant too.
- There are no continuous animations; transitions only run on value changes (progress `scaleX`,
  hover backgrounds, switch knob).
- `prefers-contrast: more` swaps in the darker / lighter text, status, chart and border tokens and
  more opaque window and card surfaces (see the "more contrast" columns above).

## Mock mode

`pnpm dev:mock` runs Vite in mode `mock`; `src/lib/transport.ts` then imports `src/mocks/transport.ts`
dynamically. A production build replaces `import.meta.env.MODE` with `"production"`, the branch is
removed and neither the mock module nor its JSON is emitted (`grep -r "i9-9880H" dist/` is empty).

| Query parameter | Values                                         | Effect                                                                                                                                                                                            |
| --------------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `platform`      | `apple` (default), `intel`                     | Apple M3 Pro (6 E + 6 P, IOReport power and per-core MHz) or Intel i9-9880H (8 / 16, SMC PCPT package power, nominal frequency)                                                                   |
| `theme`         | `light`, `dark`                                | forces `settings.theme`; omitted → `system`                                                                                                                                                       |
| `view`          | `overview` (default), `cpu-detail`, `settings` | initial view                                                                                                                                                                                      |
| `wifi`          | `off`                                          | Wi-Fi disconnected                                                                                                                                                                                |
| `power`         | `unavailable`                                  | power `unavailable` with the VM reason                                                                                                                                                            |
| `temperature`   | `unavailable`                                  | temperature `unavailable` with "Model Mac này không cung cấp cảm biến nhiệt độ CPU"                                                                                                               |
| `state`         | `loading`                                      | `get_static_info` / `get_snapshot` return `null`, no snapshots are pushed (skeletons)                                                                                                             |
| `rosetta`       | `on`                                           | `translated: true`; power, frequency and temperature unavailable with the Rosetta reason (wins over `temperature`)                                                                                |
| `login`         | `requiresApproval`, `blocked`                  | launch at login starts as `requiresApproval` (`launchAtLogin: true`) or `blocked` (`launchAtLogin: false`, reason "Hãy chép Pulsebar vào thư mục Applications rồi bật lại"); omitted → `disabled` |

Unknown values fall back to the defaults. Example:
`http://localhost:1420/?platform=intel&theme=dark&view=cpu-detail`.

Data sets (`src/mocks/*.json`, type-checked against `StaticInfo` / `Snapshot` in
`src/mocks/fixtures.test.ts`):

- `static-apple-silicon.json` / `snapshot-apple-silicon.json`: Apple M3 Pro, clusters E (CPUs 0–5)
  and P (CPUs 6–11) with per-core MHz, 36 GB RAM, disks Macintosh HD (boot, purgeable figure),
  SANDISK (removable, warn level) and "Chia sẻ" (SMB share), Wi-Fi with IPv4 and IPv6, temperature
  52.5 °C from `smc:TCMb`.
- `static-intel.json` / `snapshot-intel.json`: Intel i9-9880H, one "CPU" cluster of 16 logical
  CPUs, nominal 2 300 MHz / max 4 800 MHz, speed limit 100 %, 32 GB RAM at warn level, disks
  Macintosh HD (warn) and Backup (removable, critical), Wi-Fi IPv4 only, temperature 71 °C from
  `smc:TCXC`.

Behaviour: the first `get_snapshot` replays 59 historical snapshots (2 s apart, consecutive `seq`)
so sparklines start full, then a self-rescheduling `setTimeout` pushes a jittered snapshot every
2 s while a snapshot listener exists. Each new snapshot jitters the temperature by up to ±3 °C
around the fixture value, clamped to 30–100 °C. `cpu.cores` is filled only after
`set_detail_enabled(true)`, network shares appear only when "Hiện ổ mạng" is on (toggling it emits
`disks://changed`), and `hide_popover`, `resize_popover` and `quit_app` are no-ops.

Launch at login in the mock: `set_settings` recomputes the three launch fields from the requested
`launchAtLogin` alone, never from the status or reason the client sends. With `login=blocked` they
stay `false` / `blocked` / the reason whatever is sent; otherwise `true` gives `enabled` (or
`requiresApproval` with `login=requiresApproval`), `false` gives `disabled`, and the reason is
`null`. `open_login_items_settings` is a no-op that resolves `undefined` and does not change the
status, so the button stays visible in the `requiresApproval` scenario. The scenarios match the
`launchAtLoginBlocked` / `launchAtLoginRequiresApproval` contract fixtures in
`src/mocks/contract-settings.json` (checked in `src/mocks/fixtures.test.ts`).
