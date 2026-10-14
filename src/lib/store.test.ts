import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import contract from '../mocks/contract-snapshot.json';
import settingsContract from '../mocks/contract-settings.json';
import { COMMANDS, EVENTS } from './ipc';
import { HISTORY_CAPACITY, PopoverStore } from './store.svelte';
import type { Transport } from './transport';
import type { Settings, Snapshot, StaticInfo } from './types';

type Responder = (args: Record<string, unknown> | undefined) => unknown;

interface Call {
  command: string;
  args: Record<string, unknown> | undefined;
}

class FakeTransport implements Transport {
  readonly handlers = new Map<string, Set<(payload: unknown) => void>>();
  readonly calls: Call[] = [];
  readonly order: string[] = [];
  readonly responders = new Map<string, Responder>();

  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    this.calls.push({ command, args });
    this.order.push(`invoke:${command}`);
    const responder = this.responders.get(command);
    if (responder === undefined) {
      return Promise.resolve(null as T);
    }
    try {
      return Promise.resolve(responder(args) as T);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    this.order.push(`listen:${event}`);
    const set = this.handlers.get(event) ?? new Set();
    const wrapped = (payload: unknown) => handler(payload as T);
    set.add(wrapped);
    this.handlers.set(event, set);
    return Promise.resolve(() => set.delete(wrapped));
  }

  emit(event: string, payload: unknown): void {
    for (const handler of this.handlers.get(event) ?? []) {
      handler(payload);
    }
  }

  callsTo(command: string): Call[] {
    return this.calls.filter((call) => call.command === command);
  }

  listenerCount(): number {
    let count = 0;
    for (const set of this.handlers.values()) {
      count += set.size;
    }
    return count;
  }
}

const baseSnapshot = contract.snapshot as Snapshot;
const baseStatic = contract.staticInfo as StaticInfo;
const defaultSettings = settingsContract.defaultSettings as Settings;

function snapshotAt(seq: number, timestampMs: number, cpu = 10): Snapshot {
  return { ...baseSnapshot, seq, timestampMs, cpu: { ...baseSnapshot.cpu, totalUsagePct: cpu } };
}

function flush(): Promise<void> {
  return new Promise<void>((resolve) => queueMicrotask(() => resolve()));
}

async function settle(): Promise<void> {
  for (let index = 0; index < 10; index += 1) {
    await flush();
  }
}

function createTransport(): FakeTransport {
  const transport = new FakeTransport();
  transport.responders.set(COMMANDS.getSettings, () => defaultSettings);
  transport.responders.set(COMMANDS.getStaticInfo, () => baseStatic);
  transport.responders.set(COMMANDS.getSnapshot, () => snapshotAt(10, 1_000_000));
  return transport;
}

let warn: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

afterEach(() => {
  warn.mockRestore();
  vi.useRealTimers();
});

describe('PopoverStore lifecycle', () => {
  it('registers every listener before the initial refresh', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();

    const firstInvoke = transport.order.findIndex((entry) => entry.startsWith('invoke:'));
    const listens = transport.order.filter((entry) => entry.startsWith('listen:'));
    expect(listens).toEqual([
      `listen:${EVENTS.snapshot}`,
      `listen:${EVENTS.disksChanged}`,
      `listen:${EVENTS.popoverVisibility}`,
    ]);
    expect(transport.order.slice(0, 3)).toEqual(listens);
    expect(firstInvoke).toBe(3);
    expect(store.settings).toEqual(defaultSettings);
    expect(store.staticInfo).toEqual(baseStatic);
    expect(store.snapshot?.seq).toBe(10);
    expect(store.history.cpu).toEqual([10]);
  });

  it('schedules no timers while initialising and handling events', async () => {
    vi.useFakeTimers();
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    transport.emit(EVENTS.snapshot, snapshotAt(11, 1_002_000));
    transport.emit(EVENTS.popoverVisibility, { visible: false, view: null });
    transport.emit(EVENTS.popoverVisibility, { visible: true, view: null });
    await settle();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('removes its listeners on dispose', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    expect(transport.listenerCount()).toBe(3);
    store.dispose();
    expect(transport.listenerCount()).toBe(0);
  });

  it('enables detail sampling when it starts in the CPU detail view', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport, 'cpu-detail');
    await store.init();
    expect(transport.callsTo(COMMANDS.setDetailEnabled).map((call) => call.args)).toEqual([
      { enabled: true },
    ]);
  });
});

describe('PopoverStore events', () => {
  it('starts hidden like the pre-created popover window until Rust shows it', async () => {
    const transport = createTransport();
    transport.responders.set(COMMANDS.getSnapshot, () => null);
    const store = new PopoverStore(transport);
    await store.init();
    expect(store.visible).toBe(false);
    transport.emit(EVENTS.popoverVisibility, { visible: true, view: null });
    expect(store.visible).toBe(true);
  });

  it('stores pushed snapshots and treats them as proof of visibility', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    transport.emit(EVENTS.popoverVisibility, { visible: false, view: null });
    expect(store.visible).toBe(false);

    transport.emit(EVENTS.snapshot, snapshotAt(11, 1_002_000, 42));
    expect(store.visible).toBe(true);
    expect(store.snapshot?.seq).toBe(11);
    expect(store.history.cpu).toEqual([10, 42]);
  });

  it('does not invoke anything when the popover is hidden', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    const before = transport.calls.length;
    transport.emit(EVENTS.popoverVisibility, { visible: false, view: null });
    await settle();
    expect(store.visible).toBe(false);
    expect(transport.calls.length).toBe(before);
  });

  it('refreshes on show and keeps the previous static info when Rust returns null', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    transport.emit(EVENTS.popoverVisibility, { visible: false, view: null });
    transport.responders.set(COMMANDS.getStaticInfo, () => null);
    transport.responders.set(COMMANDS.getSnapshot, () => snapshotAt(30, 1_060_000, 55));
    const before = transport.calls.length;

    transport.emit(EVENTS.popoverVisibility, { visible: true, view: null });
    await settle();

    const refreshed = transport.calls.slice(before).map((call) => call.command);
    expect(refreshed).toEqual(
      expect.arrayContaining([COMMANDS.getStaticInfo, COMMANDS.getSnapshot, COMMANDS.getSettings]),
    );
    expect(store.visible).toBe(true);
    expect(store.staticInfo).toEqual(baseStatic);
    expect(store.snapshot?.seq).toBe(30);
  });

  it('switches to the view requested by Rust on show', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    transport.emit(EVENTS.popoverVisibility, { visible: true, view: 'settings' });
    await settle();
    expect(store.view).toBe('settings');
  });

  it('re-enables detail sampling when shown again in the CPU detail view', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    await store.setView('cpu-detail');
    transport.emit(EVENTS.popoverVisibility, { visible: false, view: null });
    transport.emit(EVENTS.popoverVisibility, { visible: true, view: null });
    await settle();
    expect(transport.callsTo(COMMANDS.setDetailEnabled).map((call) => call.args)).toEqual([
      { enabled: true },
      { enabled: true },
    ]);
  });

  it('marks disks as changing until the next snapshot arrives', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    transport.emit(EVENTS.disksChanged, null);
    expect(store.disksChanging).toBe(true);
    transport.emit(EVENTS.snapshot, snapshotAt(11, 1_002_000));
    expect(store.disksChanging).toBe(false);
  });
});

describe('PopoverStore history', () => {
  it('resets the history when the sequence jumps', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    transport.emit(EVENTS.snapshot, snapshotAt(11, 1_002_000, 20));
    transport.emit(EVENTS.snapshot, snapshotAt(13, 1_004_000, 30));
    expect(store.history.cpu).toEqual([30]);
  });

  it('resets the history when the time gap exceeds ten seconds', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    transport.emit(EVENTS.snapshot, snapshotAt(11, 1_010_000, 20));
    expect(store.history.cpu).toEqual([10, 20]);
    transport.emit(EVENTS.snapshot, snapshotAt(12, 1_020_001, 30));
    expect(store.history.cpu).toEqual([30]);
  });

  it('ignores stale or duplicate snapshots', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    transport.emit(EVENTS.snapshot, snapshotAt(10, 1_000_000, 99));
    transport.emit(EVENTS.snapshot, snapshotAt(9, 998_000, 99));
    expect(store.snapshot?.seq).toBe(10);
    expect(store.history.cpu).toEqual([10]);
  });

  it(`keeps at most ${HISTORY_CAPACITY} points`, async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    for (let step = 1; step <= HISTORY_CAPACITY + 5; step += 1) {
      transport.emit(EVENTS.snapshot, snapshotAt(10 + step, 1_000_000 + step * 2000, step));
    }
    expect(store.history.cpu).toHaveLength(HISTORY_CAPACITY);
    expect(store.history.cpu.at(-1)).toBe(HISTORY_CAPACITY + 5);
  });

  it('clears the power history while power is unavailable', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    expect(store.history.power).toEqual([8.25]);
    transport.emit(EVENTS.snapshot, {
      ...snapshotAt(11, 1_002_000),
      power: { state: 'unavailable', reason: 'x' },
    });
    expect(store.history.power).toEqual([]);
    expect(store.history.cpu).toHaveLength(2);
  });
});

describe('PopoverStore commands', () => {
  it('enables detail on entering the CPU view and disables it on leaving', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    await store.setView('cpu-detail');
    await store.setView('overview');
    expect(transport.callsTo(COMMANDS.setDetailEnabled).map((call) => call.args)).toEqual([
      { enabled: true },
      { enabled: false },
    ]);
  });

  it('replaces local settings with the value returned by Rust', async () => {
    const transport = createTransport();
    const saved: Settings = { ...defaultSettings, theme: 'dark', launchAtLogin: false };
    transport.responders.set(COMMANDS.setSettings, () => saved);
    const store = new PopoverStore(transport);
    await store.init();

    const pending = store.updateSettings({ theme: 'dark', launchAtLogin: true });
    expect(store.settings?.launchAtLogin).toBe(true);
    expect(store.settingsSaving).toBe(true);
    await pending;

    expect(transport.callsTo(COMMANDS.setSettings)[0]?.args).toEqual({
      settings: { ...defaultSettings, theme: 'dark', launchAtLogin: true },
    });
    expect(store.settings).toEqual(saved);
    expect(store.settingsSaving).toBe(false);
    expect(store.settingsError).toBe(false);
  });

  it('reverts settings and reports an error when saving fails', async () => {
    const transport = createTransport();
    transport.responders.set(COMMANDS.setSettings, () => {
      throw new Error('disk full');
    });
    const store = new PopoverStore(transport);
    await store.init();

    await store.updateSettings({ showNetworkDisks: true });

    expect(store.settings).toEqual(defaultSettings);
    expect(store.settingsError).toBe(true);
    expect(store.settingsSaving).toBe(false);
  });

  it('surfaces command failures as state and warns once per failure streak', async () => {
    const transport = createTransport();
    transport.responders.set(COMMANDS.hidePopover, () => {
      throw new Error('window gone');
    });
    const store = new PopoverStore(transport);
    await store.init();

    await expect(store.hide()).resolves.toBeUndefined();
    await store.hide();
    expect(store.actionFailed).toBe(true);
    expect(warn).toHaveBeenCalledTimes(1);

    transport.responders.set(COMMANDS.hidePopover, () => null);
    await store.hide();
    expect(store.actionFailed).toBe(false);
  });

  it('reports data failures when the initial snapshot cannot be read', async () => {
    const transport = createTransport();
    transport.responders.set(COMMANDS.getSnapshot, () => {
      throw new Error('not ready');
    });
    const store = new PopoverStore(transport);
    await store.init();
    expect(store.dataFailed).toBe(true);
    transport.emit(EVENTS.snapshot, snapshotAt(11, 1_002_000));
    expect(store.dataFailed).toBe(false);
  });

  it('requests a resize only when the clamped height changes', async () => {
    const transport = createTransport();
    const store = new PopoverStore(transport);
    await store.init();
    await store.resize(300.2);
    await store.resize(300.9);
    await store.resize(120);
    await store.resize(900);
    await store.resize(1200);
    expect(transport.callsTo(COMMANDS.resizePopover).map((call) => call.args)).toEqual([
      { height: 301 },
      { height: 200 },
      { height: 640 },
    ]);
  });
});
