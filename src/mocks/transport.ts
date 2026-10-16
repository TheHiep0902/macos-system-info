import { COMMANDS, EVENTS } from '../lib/ipc';
import type { Transport, TransportSetup } from '../lib/transport';
import type {
  CoreSample,
  PopoverView,
  PopoverVisibility,
  PowerSample,
  Settings,
  Snapshot,
  StaticInfo,
  TemperatureSample,
} from '../lib/types';
import appleSnapshotJson from './snapshot-apple-silicon.json';
import appleStaticJson from './static-apple-silicon.json';
import intelSnapshotJson from './snapshot-intel.json';
import intelStaticJson from './static-intel.json';

export interface MockScenario {
  platform: 'apple' | 'intel';
  theme: 'light' | 'dark' | null;
  view: PopoverView;
  wifi: boolean;
  powerUnavailable: boolean;
  temperatureUnavailable: boolean;
  loading: boolean;
  rosetta: boolean;
  login: LoginScenario | null;
}

export type LoginScenario = 'requiresApproval' | 'blocked';

type LaunchAtLoginFields = Pick<
  Settings,
  'launchAtLogin' | 'launchAtLoginStatus' | 'launchAtLoginReason'
>;

const TICK_MS = 2000;
const HISTORY_POINTS = 60;
const POWER_UNAVAILABLE_REASON = 'Máy ảo không cung cấp cảm biến công suất';
const TEMPERATURE_UNAVAILABLE_REASON = 'Model Mac này không cung cấp cảm biến nhiệt độ CPU';
const TEMPERATURE_SPREAD = 3;
const TEMPERATURE_MIN = 30;
const TEMPERATURE_MAX = 100;
const ROSETTA_REASON = 'Đang chạy qua Rosetta – hãy mở bản native';
const VIEWS: readonly PopoverView[] = ['overview', 'cpu-detail', 'settings'];
const LOGIN_SCENARIOS: readonly LoginScenario[] = ['requiresApproval', 'blocked'];
const LOGIN_BLOCKED_REASON = 'Hãy chép Pulsebar vào thư mục Applications rồi bật lại';

const DEFAULT_SETTINGS: Settings = {
  titleFormat: 'default',
  showTrayIcon: true,
  titleMetrics: { cpu: true, power: true, memory: true, temperature: false },
  theme: 'system',
  reduceEffects: false,
  showNetworkDisks: false,
  memorySaver: false,
  launchAtLogin: false,
  launchAtLoginStatus: 'disabled',
  launchAtLoginReason: null,
};

function pick<T extends string>(value: string | null, allowed: readonly T[]): T | null {
  return allowed.find((candidate) => candidate === value) ?? null;
}

export function parseScenario(params: URLSearchParams): MockScenario {
  return {
    platform: pick(params.get('platform'), ['apple', 'intel'] as const) ?? 'apple',
    theme: pick(params.get('theme'), ['light', 'dark'] as const),
    view: pick(params.get('view'), VIEWS) ?? 'overview',
    wifi: params.get('wifi') !== 'off',
    powerUnavailable: params.get('power') === 'unavailable',
    temperatureUnavailable: params.get('temperature') === 'unavailable',
    loading: params.get('state') === 'loading',
    rosetta: params.get('rosetta') === 'on',
    login: pick(params.get('login'), LOGIN_SCENARIOS),
  };
}

function launchAtLoginFields(login: LoginScenario | null, requested: boolean): LaunchAtLoginFields {
  if (login === 'blocked') {
    return {
      launchAtLogin: false,
      launchAtLoginStatus: 'blocked',
      launchAtLoginReason: LOGIN_BLOCKED_REASON,
    };
  }
  if (!requested) {
    return { launchAtLogin: false, launchAtLoginStatus: 'disabled', launchAtLoginReason: null };
  }
  return {
    launchAtLogin: true,
    launchAtLoginStatus: login === 'requiresApproval' ? 'requiresApproval' : 'enabled',
    launchAtLoginReason: null,
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function round2(value: number): number {
  return Math.round(value * 100) / 100;
}

function jitter(value: number, spread: number, min: number, max: number): number {
  return round2(clamp(value + (Math.random() * 2 - 1) * spread, min, max));
}

function average(values: readonly number[]): number {
  return values.length === 0 ? 0 : values.reduce((sum, value) => sum + value, 0) / values.length;
}

class MockBackend {
  readonly #scenario: MockScenario;
  readonly #staticInfo: StaticInfo;
  readonly #handlers = new Map<string, Set<(payload: unknown) => void>>();
  #settings: Settings;
  #current: Snapshot;
  readonly #temperatureBase: number | null;
  #detail = false;
  #seeded = false;
  #timer: ReturnType<typeof setTimeout> | null = null;

  constructor(scenario: MockScenario) {
    this.#scenario = scenario;
    const apple = scenario.platform === 'apple';
    const staticInfo = (apple ? appleStaticJson : intelStaticJson) as StaticInfo;
    this.#staticInfo = { ...staticInfo, translated: scenario.rosetta };
    this.#settings = {
      ...DEFAULT_SETTINGS,
      theme: scenario.theme ?? DEFAULT_SETTINGS.theme,
      ...launchAtLoginFields(scenario.login, scenario.login === 'requiresApproval'),
    };
    const snapshot = (apple ? appleSnapshotJson : intelSnapshotJson) as Snapshot;
    this.#temperatureBase =
      snapshot.temperature.state === 'ok' ? snapshot.temperature.celsius : null;
    this.#current = this.#decorate(snapshot);
  }

  readonly transport: Transport = {
    invoke: <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      try {
        return Promise.resolve(this.#invoke(command, args) as T);
      } catch (error) {
        return Promise.reject(error);
      }
    },
    listen: <T>(event: string, handler: (payload: T) => void): Promise<() => void> => {
      const handlers = this.#handlers.get(event) ?? new Set();
      const wrapped = (payload: unknown) => handler(payload as T);
      handlers.add(wrapped);
      this.#handlers.set(event, handlers);
      return Promise.resolve(() => {
        handlers.delete(wrapped);
        if (event === EVENTS.snapshot && handlers.size === 0) {
          this.#stop();
        }
      });
    },
  };

  #invoke(command: string, args: Record<string, unknown> | undefined): unknown {
    switch (command) {
      case COMMANDS.getStaticInfo:
        return this.#scenario.loading ? null : this.#staticInfo;
      case COMMANDS.getSnapshot:
        return this.#getSnapshot();
      case COMMANDS.setDetailEnabled:
        this.#detail = args?.['enabled'] === true;
        return null;
      case COMMANDS.getSettings:
        return { ...this.#settings };
      case COMMANDS.setSettings:
        return this.#setSettings(args?.['settings']);
      case COMMANDS.hidePopover:
      case COMMANDS.resizePopover:
      case COMMANDS.quitApp:
        return null;
      case COMMANDS.openLoginItemsSettings:
        return undefined;
      default:
        throw new Error(`mock: unknown command ${command}`);
    }
  }

  #getSnapshot(): Snapshot | null {
    if (this.#scenario.loading) {
      return null;
    }
    if (!this.#seeded) {
      this.#seeded = true;
      this.#seedHistory();
      this.#schedule();
    }
    return this.#view(this.#current);
  }

  #setSettings(value: unknown): Settings {
    if (typeof value !== 'object' || value === null) {
      throw new Error('mock: settings must be an object');
    }
    const merged: Settings = { ...this.#settings, ...(value as Partial<Settings>) };
    const next: Settings = {
      ...merged,
      ...launchAtLoginFields(this.#scenario.login, merged.launchAtLogin === true),
    };
    const disksChanged = next.showNetworkDisks !== this.#settings.showNetworkDisks;
    this.#settings = next;
    if (disksChanged) {
      this.#emit(EVENTS.disksChanged, null);
    }
    return { ...next };
  }

  #seedHistory(): void {
    const latest = this.#current;
    let previous: Snapshot = {
      ...latest,
      seq: latest.seq - HISTORY_POINTS + 1,
      timestampMs: Date.now() - (HISTORY_POINTS - 1) * TICK_MS,
    };
    this.#emit(EVENTS.snapshot, this.#view(previous));
    for (let step = 1; step < HISTORY_POINTS; step += 1) {
      previous = this.#next(previous, previous.timestampMs + TICK_MS);
      this.#emit(EVENTS.snapshot, this.#view(previous));
    }
    this.#current = previous;
  }

  #schedule(): void {
    this.#timer = setTimeout(() => {
      this.#timer = null;
      this.#current = this.#next(this.#current, Date.now());
      this.#emit(EVENTS.snapshot, this.#view(this.#current));
      this.#schedule();
    }, TICK_MS);
  }

  #stop(): void {
    if (this.#timer !== null) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
  }

  #emit(event: string, payload: Snapshot | PopoverVisibility | null): void {
    for (const handler of this.#handlers.get(event) ?? []) {
      handler(payload);
    }
  }

  #decorate(snapshot: Snapshot): Snapshot {
    const power: Snapshot['power'] = this.#scenario.rosetta
      ? { state: 'unavailable', reason: ROSETTA_REASON }
      : this.#scenario.powerUnavailable
        ? { state: 'unavailable', reason: POWER_UNAVAILABLE_REASON }
        : snapshot.power;
    const temperature: Snapshot['temperature'] = this.#scenario.rosetta
      ? { state: 'unavailable', reason: ROSETTA_REASON }
      : this.#scenario.temperatureUnavailable
        ? { state: 'unavailable', reason: TEMPERATURE_UNAVAILABLE_REASON }
        : snapshot.temperature;
    const frequency: Snapshot['cpu']['frequency'] = this.#scenario.rosetta
      ? { state: 'unavailable', reason: ROSETTA_REASON }
      : snapshot.cpu.frequency;
    const network: Snapshot['network'] = this.#scenario.wifi
      ? snapshot.network
      : {
          wifiInterface: 'en0',
          wifiIpv4: null,
          wifiIpv6: null,
          isWifiConnected: false,
          primaryInterface: null,
        };
    return { ...snapshot, power, temperature, network, cpu: { ...snapshot.cpu, frequency } };
  }

  #next(previous: Snapshot, timestampMs: number): Snapshot {
    const cores = previous.cpu.cores.map((core): CoreSample => {
      const usagePct = jitter(core.usagePct, 6, 1, 99);
      const ceiling =
        this.#staticInfo.clusters.find((cluster) => cluster.label === core.cluster)?.maxMhz ??
        this.#staticInfo.maxMhz ??
        4000;
      const freqMhz =
        core.freqMhz === null ? null : Math.round(jitter(core.freqMhz, 180, 600, ceiling));
      return {
        ...core,
        usagePct,
        freqMhz,
        activeRatio: core.activeRatio === null ? null : round2(clamp(usagePct / 100 + 0.1, 0, 1)),
      };
    });
    const clusters = previous.cpu.clusters.map((cluster) => {
      const members = cores.filter((core) => core.cluster === cluster.label);
      const mhz = members.flatMap((core) => (core.freqMhz === null ? [] : [core.freqMhz]));
      return {
        ...cluster,
        usagePct: round2(average(members.map((core) => core.usagePct))),
        freqMhz: mhz.length === 0 ? null : Math.round(average(mhz)),
      };
    });
    const allMhz = cores.flatMap((core) => (core.freqMhz === null ? [] : [core.freqMhz]));
    const frequency: Snapshot['cpu']['frequency'] =
      previous.cpu.frequency.state === 'ok' && allMhz.length > 0
        ? {
            ...previous.cpu.frequency,
            avgMhz: Math.round(average(allMhz)),
            minMhz: Math.min(...allMhz),
            maxMhz: Math.max(...allMhz),
          }
        : previous.cpu.frequency;
    const power: Snapshot['power'] =
      previous.power.state === 'ok' ? this.#nextPower(previous.power) : previous.power;
    const temperature: Snapshot['temperature'] =
      previous.temperature.state === 'ok'
        ? this.#nextTemperature(previous.temperature)
        : previous.temperature;
    const usedPct = jitter(previous.memory.usedPct, 0.6, 20, 98);
    const usedBytes = Math.round((previous.memory.totalBytes * usedPct) / 100);
    return {
      ...previous,
      seq: previous.seq + 1,
      timestampMs,
      uptimeSecs: previous.uptimeSecs + TICK_MS / 1000,
      cpu: {
        totalUsagePct: round2(average(cores.map((core) => core.usagePct))),
        frequency,
        clusters,
        cores,
      },
      power,
      temperature,
      memory: {
        ...previous.memory,
        usedPct,
        usedBytes,
        appBytes: Math.max(
          0,
          usedBytes - previous.memory.wiredBytes - previous.memory.compressedBytes,
        ),
      },
    };
  }

  #nextPower(power: PowerSample): Snapshot['power'] {
    const cpuWatts = jitter(power.cpuWatts, 1.5, 0.5, 45);
    return {
      ...power,
      state: 'ok',
      cpuWatts,
      smoothedCpuWatts: round2(power.smoothedCpuWatts * 0.7 + cpuWatts * 0.3),
    };
  }

  #nextTemperature(temperature: TemperatureSample): Snapshot['temperature'] {
    const base = this.#temperatureBase ?? temperature.celsius;
    return {
      ...temperature,
      state: 'ok',
      celsius: jitter(base, TEMPERATURE_SPREAD, TEMPERATURE_MIN, TEMPERATURE_MAX),
    };
  }

  #view(snapshot: Snapshot): Snapshot {
    const disks = this.#settings.showNetworkDisks
      ? snapshot.disks
      : snapshot.disks.filter((disk) => !disk.isNetwork);
    return {
      ...snapshot,
      disks,
      cpu: { ...snapshot.cpu, cores: this.#detail ? snapshot.cpu.cores : [] },
    };
  }
}

export function createMockTransport(params: URLSearchParams): TransportSetup {
  const scenario = parseScenario(params);
  const backend = new MockBackend(scenario);
  return { transport: backend.transport, initialView: scenario.view };
}
