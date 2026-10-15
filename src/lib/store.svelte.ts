import { COMMANDS, EVENTS, POPOVER_MAX_HEIGHT, POPOVER_MIN_HEIGHT } from './ipc';
import type { Transport } from './transport';
import type { PopoverView, PopoverVisibility, Settings, Snapshot, StaticInfo } from './types';

export const HISTORY_CAPACITY = 60;

export const HISTORY_MAX_GAP_MS = 10_000;

const ERROR_TEXT_LIMIT = 200;

export type CommandName = (typeof COMMANDS)[keyof typeof COMMANDS];

export interface History {
  readonly cpu: readonly number[];
  readonly power: readonly number[];
  readonly temperature: readonly number[];
  readonly memory: readonly number[];
}

type CallResult<T> = { ok: true; value: T } | { ok: false };

const EMPTY_HISTORY: History = { cpu: [], power: [], temperature: [], memory: [] };

const DATA_COMMANDS: readonly CommandName[] = [COMMANDS.getSnapshot, COMMANDS.getStaticInfo];

const ACTION_COMMANDS: readonly CommandName[] = [
  COMMANDS.hidePopover,
  COMMANDS.quitApp,
  COMMANDS.setDetailEnabled,
];

function appendBounded(values: readonly number[], value: number): readonly number[] {
  const kept = values.length >= HISTORY_CAPACITY ? values.slice(1 - HISTORY_CAPACITY) : values;
  return [...kept, value];
}

function isNewer(previous: Snapshot, next: Snapshot): boolean {
  return next.seq > previous.seq || next.timestampMs > previous.timestampMs;
}

function isContinuous(previous: Snapshot, next: Snapshot): boolean {
  const gap = next.timestampMs - previous.timestampMs;
  return next.seq - previous.seq === 1 && gap >= 0 && gap <= HISTORY_MAX_GAP_MS;
}

function describeError(error: unknown): string {
  const text =
    error instanceof Error ? error.message : typeof error === 'string' ? error : 'unknown error';
  return text.slice(0, ERROR_TEXT_LIMIT);
}

export class PopoverStore {
  view = $state<PopoverView>('overview');
  visible = $state(false);
  staticInfo = $state.raw<StaticInfo | null>(null);
  snapshot = $state.raw<Snapshot | null>(null);
  settings = $state.raw<Settings | null>(null);
  history = $state.raw<History>(EMPTY_HISTORY);
  disksChanging = $state(false);
  settingsSaving = $state(false);
  settingsError = $state(false);
  liveUpdatesFailed = $state(false);
  failures = $state.raw<readonly CommandName[]>([]);

  readonly #transport: Transport;
  readonly #unlisteners: (() => void)[] = [];
  #disposed = false;
  #lastHeight: number | null = null;
  #confirmedSettings: Settings | null = null;
  #pendingSaves = 0;
  #settingsQueue: Promise<void> = Promise.resolve();

  constructor(transport: Transport, initialView: PopoverView = 'overview') {
    this.#transport = transport;
    this.view = initialView;
  }

  get dataFailed(): boolean {
    return (
      this.liveUpdatesFailed || DATA_COMMANDS.some((command) => this.failures.includes(command))
    );
  }

  get actionFailed(): boolean {
    return ACTION_COMMANDS.some((command) => this.failures.includes(command));
  }

  get settingsLoadFailed(): boolean {
    return this.settings === null && this.failures.includes(COMMANDS.getSettings);
  }

  async init(): Promise<void> {
    await this.#subscribe();
    if (this.#disposed) {
      return;
    }
    await this.refresh();
    if (!this.#disposed && this.view === 'cpu-detail') {
      await this.#setDetail(true);
    }
  }

  dispose(): void {
    this.#disposed = true;
    for (const unlisten of this.#unlisteners.splice(0)) {
      unlisten();
    }
  }

  async refresh(): Promise<void> {
    const [settings, staticInfo, snapshot] = await Promise.all([
      this.#call<Settings>(COMMANDS.getSettings),
      this.#call<StaticInfo | null>(COMMANDS.getStaticInfo),
      this.#call<Snapshot | null>(COMMANDS.getSnapshot),
    ]);
    if (this.#disposed) {
      return;
    }
    if (settings.ok && this.#pendingSaves === 0) {
      this.settings = settings.value;
      this.#confirmedSettings = settings.value;
    }
    if (staticInfo.ok && staticInfo.value !== null) {
      this.staticInfo = staticInfo.value;
    }
    if (snapshot.ok && snapshot.value !== null) {
      this.#applySnapshot(snapshot.value);
    }
  }

  async setView(next: PopoverView): Promise<void> {
    const previous = this.view;
    if (previous === next) {
      return;
    }
    this.view = next;
    if (next === 'cpu-detail') {
      await this.#setDetail(true);
    } else if (previous === 'cpu-detail') {
      await this.#setDetail(false);
    }
  }

  updateSettings(patch: Partial<Settings>): Promise<void> {
    const current = this.settings;
    if (current === null) {
      return Promise.resolve();
    }
    this.settings = { ...current, ...patch };
    this.settingsError = false;
    this.#pendingSaves += 1;
    this.settingsSaving = true;
    const run = this.#settingsQueue.then(() => this.#persistSettings());
    this.#settingsQueue = run;
    return run;
  }

  async hide(): Promise<void> {
    await this.#call<void>(COMMANDS.hidePopover);
  }

  async quit(): Promise<void> {
    await this.#call<void>(COMMANDS.quitApp);
  }

  async resize(contentHeight: number): Promise<void> {
    if (!Number.isFinite(contentHeight)) {
      return;
    }
    const height = Math.min(
      POPOVER_MAX_HEIGHT,
      Math.max(POPOVER_MIN_HEIGHT, Math.ceil(contentHeight)),
    );
    if (height === this.#lastHeight) {
      return;
    }
    this.#lastHeight = height;
    const result = await this.#call<void>(COMMANDS.resizePopover, { height });
    if (!result.ok && this.#lastHeight === height) {
      this.#lastHeight = null;
    }
  }

  async #subscribe(): Promise<void> {
    const results = await Promise.allSettled([
      this.#transport.listen<Snapshot>(EVENTS.snapshot, (payload) => this.#onSnapshot(payload)),
      this.#transport.listen<null>(EVENTS.disksChanged, () => this.#onDisksChanged()),
      this.#transport.listen<PopoverVisibility>(EVENTS.popoverVisibility, (payload) =>
        this.#onVisibility(payload),
      ),
    ]);
    for (const result of results) {
      if (result.status === 'rejected') {
        this.liveUpdatesFailed = true;
        console.warn(`[pulsebar] event listener failed: ${describeError(result.reason)}`);
      } else if (this.#disposed) {
        result.value();
      } else {
        this.#unlisteners.push(result.value);
      }
    }
  }

  #onSnapshot(snapshot: Snapshot): void {
    if (this.#disposed) {
      return;
    }
    this.visible = true;
    this.#applySnapshot(snapshot);
  }

  #onDisksChanged(): void {
    if (this.#disposed) {
      return;
    }
    this.disksChanging = true;
  }

  #onVisibility(visibility: PopoverVisibility): void {
    if (this.#disposed) {
      return;
    }
    if (!visibility.visible) {
      this.visible = false;
      return;
    }
    this.visible = true;
    if (visibility.view !== null) {
      void this.setView(visibility.view);
    } else if (this.view === 'cpu-detail') {
      void this.#setDetail(true);
    }
    void this.refresh();
  }

  #applySnapshot(next: Snapshot): void {
    const previous = this.snapshot;
    if (previous !== null && !isNewer(previous, next)) {
      return;
    }
    const base = previous !== null && isContinuous(previous, next) ? this.history : EMPTY_HISTORY;
    this.history = {
      cpu: appendBounded(base.cpu, next.cpu.totalUsagePct),
      memory: appendBounded(base.memory, next.memory.usedPct),
      power:
        next.power.state === 'ok' ? appendBounded(base.power, next.power.smoothedCpuWatts) : [],
      temperature:
        next.temperature.state === 'ok'
          ? appendBounded(base.temperature, next.temperature.celsius)
          : [],
    };
    this.snapshot = next;
    this.disksChanging = false;
    this.#clearFailure(COMMANDS.getSnapshot);
  }

  async #persistSettings(): Promise<void> {
    const desired = this.settings;
    if (desired !== null && desired !== this.#confirmedSettings && !this.#disposed) {
      const result = await this.#call<Settings>(COMMANDS.setSettings, { settings: desired });
      if (result.ok) {
        this.#confirmedSettings = result.value;
        if (this.settings === desired) {
          this.settings = result.value;
        }
      } else {
        this.settings = this.#confirmedSettings;
        this.settingsError = true;
      }
    }
    this.#pendingSaves -= 1;
    this.settingsSaving = this.#pendingSaves > 0;
  }

  async #setDetail(enabled: boolean): Promise<void> {
    await this.#call<void>(COMMANDS.setDetailEnabled, { enabled });
  }

  async #call<T>(command: CommandName, args?: Record<string, unknown>): Promise<CallResult<T>> {
    try {
      const value = await this.#transport.invoke<T>(command, args);
      this.#clearFailure(command);
      return { ok: true, value };
    } catch (error) {
      if (!this.failures.includes(command)) {
        console.warn(`[pulsebar] ${command} failed: ${describeError(error)}`);
        this.failures = [...this.failures, command];
      }
      return { ok: false };
    }
  }

  #clearFailure(command: CommandName): void {
    if (this.failures.includes(command)) {
      this.failures = this.failures.filter((failed) => failed !== command);
    }
  }
}
