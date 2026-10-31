import { afterEach, describe, expect, it, vi } from 'vitest';
import { COMMANDS, EVENTS } from '../lib/ipc';
import type { Settings, Snapshot, StaticInfo, UpdateState } from '../lib/types';
import settingsContract from './contract-settings.json';
import updateContract from './contract-update.json';
import appleSnapshot from './snapshot-apple-silicon.json';
import intelSnapshot from './snapshot-intel.json';
import appleStatic from './static-apple-silicon.json';
import intelStatic from './static-intel.json';
import {
  UPDATE_CHECK_MS,
  UPDATE_TICK_MS,
  UPDATE_TOTAL_BYTES,
  createMockTransport,
  parseScenario,
} from './transport';

type JsonShape<T> = T extends string
  ? string
  : T extends number
    ? number
    : T extends boolean
      ? boolean
      : T extends null
        ? null
        : T extends readonly (infer Item)[]
          ? JsonShape<Item>[]
          : T extends object
            ? { [Key in keyof T]: JsonShape<T[Key]> }
            : never;

const typedFixtures = {
  appleSnapshot: appleSnapshot satisfies JsonShape<Snapshot>,
  intelSnapshot: intelSnapshot satisfies JsonShape<Snapshot>,
  appleStatic: appleStatic satisfies JsonShape<StaticInfo>,
  intelStatic: intelStatic satisfies JsonShape<StaticInfo>,
};

const FREQUENCY_STATES = ['ok', 'unavailable', 'loading'];
const FREQ_SOURCES = ['ioReport', 'nominal'];
const TEMPERATURE_SOURCE = /^(smc:[A-Za-z0-9]{4}|macmon)$/;

function expectSnapshotLiterals(snapshot: JsonShape<Snapshot>): void {
  expect(FREQUENCY_STATES).toContain(snapshot.cpu.frequency.state);
  expect(FREQUENCY_STATES).toContain(snapshot.power.state);
  expect(FREQUENCY_STATES).toContain(snapshot.temperature.state);
  if ('source' in snapshot.cpu.frequency) {
    expect(FREQ_SOURCES).toContain(snapshot.cpu.frequency.source);
  }
  if ('source' in snapshot.temperature) {
    expect(snapshot.temperature.source).toMatch(TEMPERATURE_SOURCE);
  }
}

describe('mock fixtures', () => {
  it('describe an Apple M3 Pro with 6 E and 6 P cores', () => {
    const { appleStatic: info, appleSnapshot: snapshot } = typedFixtures;
    expect(info.platform).toBe('appleSilicon');
    expect(info.cpuBrand).toBe('Apple M3 Pro');
    expect(info.clusters.map((cluster) => [cluster.label, cluster.coreCount])).toEqual([
      ['E', 6],
      ['P', 6],
    ]);
    expect(snapshot.cpu.cores.map((core) => core.cluster)).toEqual([
      ...Array<string>(6).fill('E'),
      ...Array<string>(6).fill('P'),
    ]);
    expect(snapshot.cpu.cores.every((core) => typeof core.freqMhz === 'number')).toBe(true);
    expect(snapshot.cpu.frequency).toMatchObject({
      state: 'ok',
      source: 'ioReport',
      perCoreAvailable: true,
    });
    expect(snapshot.power).toMatchObject({ state: 'ok', source: 'ioreport', scope: 'cpu' });
    expect(snapshot.temperature).toEqual({ state: 'ok', celsius: 52.5, source: 'smc:TCMb' });
    expectSnapshotLiterals(snapshot);
  });

  it('describe an Intel i9-9880H with nominal frequency only', () => {
    const { intelStatic: info, intelSnapshot: snapshot } = typedFixtures;
    expect(info.platform).toBe('intel');
    expect(info.cpuBrand).toBe('Intel(R) Core(TM) i9-9880H CPU @ 2.30GHz');
    expect([info.physicalCores, info.logicalCores]).toEqual([8, 16]);
    expect([info.nominalMhz, info.maxMhz]).toEqual([2300, 4800]);
    expect(info.clusters.map((cluster) => cluster.label)).toEqual(['CPU']);
    expect(snapshot.cpu.cores).toHaveLength(16);
    expect(snapshot.cpu.frequency).toEqual({
      state: 'ok',
      source: 'nominal',
      avgMhz: null,
      minMhz: null,
      maxMhz: null,
      perCoreAvailable: false,
      speedLimitPct: 100,
    });
    expect(snapshot.power).toMatchObject({ state: 'ok', source: 'smc:PCPT', scope: 'package' });
    expect(snapshot.temperature).toEqual({ state: 'ok', celsius: 71, source: 'smc:TCXC' });
    expectSnapshotLiterals(snapshot);
  });

  it('include a boot volume, a removable volume, a Wi-Fi connection and throughput', () => {
    for (const snapshot of [typedFixtures.appleSnapshot, typedFixtures.intelSnapshot]) {
      expect(snapshot.throughput).toMatchObject({ state: 'ok', interfaces: ['en0'] });
      expect(snapshot.disks.some((disk) => disk.isBoot)).toBe(true);
      expect(snapshot.disks.some((disk) => disk.isRemovable)).toBe(true);
      expect(snapshot.network.isWifiConnected).toBe(true);
    }
  });
});

describe('parseScenario', () => {
  it('reads every supported query parameter', () => {
    const scenario = parseScenario(
      new URLSearchParams(
        'platform=intel&theme=dark&view=cpu-detail&wifi=off&power=unavailable&temperature=unavailable&throughput=unavailable&state=loading&rosetta=on&login=requiresApproval&update=post',
      ),
    );
    expect(scenario).toEqual({
      platform: 'intel',
      theme: 'dark',
      view: 'cpu-detail',
      wifi: false,
      powerUnavailable: true,
      temperatureUnavailable: true,
      throughput: 'unavailable',
      loading: true,
      rosetta: true,
      login: 'requiresApproval',
      update: 'post',
    });
  });

  it('reads the loading throughput scenario', () => {
    expect(parseScenario(new URLSearchParams('throughput=loading')).throughput).toBe('loading');
  });

  it('reads the blocked launch-at-login scenario', () => {
    expect(parseScenario(new URLSearchParams('login=blocked')).login).toBe('blocked');
  });

  it('falls back to defaults for unknown values', () => {
    expect(
      parseScenario(
        new URLSearchParams(
          'platform=arm&theme=blue&view=../x&temperature=off&throughput=on&login=enabled',
        ),
      ),
    ).toEqual({
      platform: 'apple',
      theme: null,
      view: 'overview',
      wifi: true,
      powerUnavailable: false,
      temperatureUnavailable: false,
      throughput: 'ok',
      loading: false,
      rosetta: false,
      login: null,
      update: 'idle',
    });
  });
});

describe('mock backend launch at login', () => {
  type LaunchFields = Pick<
    Settings,
    'launchAtLogin' | 'launchAtLoginStatus' | 'launchAtLoginReason'
  >;

  const DISABLED: LaunchFields = {
    launchAtLogin: false,
    launchAtLoginStatus: 'disabled',
    launchAtLoginReason: null,
  };
  const ENABLED: LaunchFields = {
    launchAtLogin: true,
    launchAtLoginStatus: 'enabled',
    launchAtLoginReason: null,
  };
  const BLOCKED = settingsContract.launchAtLoginBlocked as LaunchFields;
  const REQUIRES_APPROVAL = settingsContract.launchAtLoginRequiresApproval as LaunchFields;

  function launchFields(settings: Settings): LaunchFields {
    return {
      launchAtLogin: settings.launchAtLogin,
      launchAtLoginStatus: settings.launchAtLoginStatus,
      launchAtLoginReason: settings.launchAtLoginReason,
    };
  }

  async function initial(query: string): Promise<Settings> {
    const { transport } = createMockTransport(new URLSearchParams(query));
    return transport.invoke<Settings>(COMMANDS.getSettings);
  }

  async function save(query: string, patch: Partial<Settings>): Promise<Settings> {
    const { transport } = createMockTransport(new URLSearchParams(query));
    const current = await transport.invoke<Settings>(COMMANDS.getSettings);
    return transport.invoke<Settings>(COMMANDS.setSettings, {
      settings: { ...current, ...patch },
    });
  }

  it.each([
    ['', DISABLED],
    ['login=requiresApproval', REQUIRES_APPROVAL],
    ['login=blocked', BLOCKED],
  ])('starts from the scenario %j', async (query, expected) => {
    expect(launchFields(await initial(query))).toEqual(expected);
  });

  it.each([
    ['', true, ENABLED],
    ['', false, DISABLED],
    ['login=requiresApproval', true, REQUIRES_APPROVAL],
    ['login=requiresApproval', false, DISABLED],
    ['login=blocked', true, BLOCKED],
    ['login=blocked', false, BLOCKED],
  ])(
    'recomputes the status in scenario %j when launchAtLogin is %s',
    async (query, on, expected) => {
      expect(launchFields(await save(query, { launchAtLogin: on }))).toEqual(expected);
    },
  );

  it('never trusts the output-only status and reason sent by the client', async () => {
    const forged: Partial<Settings> = {
      launchAtLoginStatus: 'blocked',
      launchAtLoginReason: 'forged',
    };
    expect(launchFields(await save('', { ...forged, launchAtLogin: true }))).toEqual(ENABLED);
    expect(launchFields(await save('', { ...forged, launchAtLogin: false }))).toEqual(DISABLED);
    expect(
      launchFields(
        await save('login=blocked', {
          launchAtLogin: true,
          launchAtLoginStatus: 'enabled',
          launchAtLoginReason: null,
        }),
      ),
    ).toEqual(BLOCKED);
  });

  it('keeps the recomputed status for the next get_settings', async () => {
    const { transport } = createMockTransport(new URLSearchParams('login=requiresApproval'));
    const current = await transport.invoke<Settings>(COMMANDS.getSettings);
    await transport.invoke(COMMANDS.setSettings, {
      settings: { ...current, launchAtLogin: false },
    });
    expect(launchFields(await transport.invoke<Settings>(COMMANDS.getSettings))).toEqual(DISABLED);
  });

  it('treats open_login_items_settings as a no-op', async () => {
    const { transport } = createMockTransport(new URLSearchParams('login=requiresApproval'));
    await expect(transport.invoke(COMMANDS.openLoginItemsSettings)).resolves.toBeUndefined();
    expect(launchFields(await transport.invoke<Settings>(COMMANDS.getSettings))).toEqual(
      REQUIRES_APPROVAL,
    );
  });
});

describe('mock backend temperature', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  async function seededSnapshots(query: string): Promise<Snapshot[]> {
    vi.useFakeTimers();
    const { transport } = createMockTransport(new URLSearchParams(query));
    const pushed: Snapshot[] = [];
    const unlisten = await transport.listen<Snapshot>(EVENTS.snapshot, (snapshot) => {
      pushed.push(snapshot);
    });
    const latest = await transport.invoke<Snapshot>(COMMANDS.getSnapshot);
    unlisten();
    expect(vi.getTimerCount()).toBe(0);
    return [...pushed, latest];
  }

  it('reports the missing-sensor reason for temperature=unavailable', async () => {
    const snapshots = await seededSnapshots('temperature=unavailable');
    for (const snapshot of snapshots) {
      expect(snapshot.temperature).toEqual({
        state: 'unavailable',
        reason: 'Model Mac này không cung cấp cảm biến nhiệt độ CPU',
      });
    }
  });

  it('reports the Rosetta reason for temperature, power and frequency under Rosetta', async () => {
    const reason = 'Đang chạy qua Rosetta – hãy mở bản native';
    const [snapshot] = await seededSnapshots('platform=intel&rosetta=on&temperature=unavailable');
    expect(snapshot?.temperature).toEqual({ state: 'unavailable', reason });
    expect(snapshot?.power).toEqual({ state: 'unavailable', reason });
    expect(snapshot?.cpu.frequency).toEqual({ state: 'unavailable', reason });
  });

  it.each([
    [0, 49.5],
    [1, 55.5],
  ])('jitters around the fixture value (random %s gives %s °C)', async (random, celsius) => {
    vi.spyOn(Math, 'random').mockReturnValue(random);
    const snapshots = await seededSnapshots('platform=apple');
    expect(snapshots).toHaveLength(61);
    for (const snapshot of snapshots.slice(1)) {
      expect(snapshot.temperature).toEqual({ state: 'ok', celsius, source: 'smc:TCMb' });
    }
  });
});

describe('mock backend throughput', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  async function seededSnapshots(query: string): Promise<Snapshot[]> {
    vi.useFakeTimers();
    const { transport } = createMockTransport(new URLSearchParams(query));
    const pushed: Snapshot[] = [];
    const unlisten = await transport.listen<Snapshot>(EVENTS.snapshot, (snapshot) => {
      pushed.push(snapshot);
    });
    const latest = await transport.invoke<Snapshot>(COMMANDS.getSnapshot);
    unlisten();
    return [...pushed, latest];
  }

  it.each([
    [0, 840_000, 62_000],
    [1, 15_960_000, 1_178_000],
  ])('varies around the fixture rates (random %s)', async (random, rx, tx) => {
    vi.spyOn(Math, 'random').mockReturnValue(random);
    const snapshots = await seededSnapshots('platform=apple');
    for (const snapshot of snapshots.slice(1)) {
      expect(snapshot.throughput).toEqual({
        state: 'ok',
        rxBytesPerSec: rx,
        txBytesPerSec: tx,
        interfaces: ['en0'],
      });
    }
  });

  it.each([
    ['throughput=unavailable', { state: 'unavailable', reason: 'Không đọc được lưu lượng mạng' }],
    ['throughput=loading', { state: 'loading' }],
  ])('keeps the %s state on every tick', async (query, expected) => {
    for (const snapshot of await seededSnapshots(query)) {
      expect(snapshot.throughput).toEqual(expected);
    }
  });
});

describe('mock backend updater', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  async function setup(query: string) {
    const { transport } = createMockTransport(new URLSearchParams(query));
    const pushed: UpdateState[] = [];
    await transport.listen<UpdateState>(EVENTS.updaterState, (state) => {
      pushed.push(state);
    });
    return { transport, pushed };
  }

  it.each([
    ['', updateContract.idle],
    ['update=uptodate', updateContract.upToDate],
    ['update=available', updateContract.available],
    ['update=downloading', updateContract.downloading],
    ['update=installing', updateContract.installing],
    ['update=failed', updateContract.failed],
    ['update=blocked', updateContract.blocked],
    ['update=post', updateContract.postUpdate],
    ['update=bogus', updateContract.idle],
  ])('seeds the scenario %j from the contract fixture', async (query, expected) => {
    const { transport } = await setup(query);
    expect(await transport.invoke<UpdateState>(COMMANDS.getUpdateState)).toEqual({
      ...(expected as UpdateState),
      autoCheck: true,
    });
  });

  it('goes from checking to available after a short delay', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(5_000_000);
    const { transport, pushed } = await setup('');
    const returned = await transport.invoke<UpdateState>(COMMANDS.checkForUpdates);
    expect(returned.phase).toEqual({ kind: 'checking' });
    vi.advanceTimersByTime(UPDATE_CHECK_MS);
    expect(pushed.map((state) => state.phase.kind)).toEqual(['checking', 'available']);
    expect(pushed.at(-1)?.lastCheckAt).toBe(5_000_000 + UPDATE_CHECK_MS);
  });

  it('reports up to date in the uptodate scenario and ignores a check while busy', async () => {
    vi.useFakeTimers();
    const { transport, pushed } = await setup('update=uptodate');
    await transport.invoke(COMMANDS.checkForUpdates);
    await transport.invoke(COMMANDS.checkForUpdates);
    vi.advanceTimersByTime(UPDATE_CHECK_MS);
    expect(pushed.map((state) => state.phase.kind)).toEqual(['checking', 'upToDate']);
  });

  it('downloads with progress ticks, then stays installing', async () => {
    vi.useFakeTimers();
    const { transport, pushed } = await setup('update=available');
    const returned = await transport.invoke<UpdateState>(COMMANDS.installUpdate);
    expect(returned.phase).toEqual({ kind: 'downloading', received: 0, total: UPDATE_TOTAL_BYTES });
    vi.advanceTimersByTime(UPDATE_TICK_MS * 10);
    const kinds = pushed.map((state) => state.phase.kind);
    expect(kinds.at(-1)).toBe('installing');
    expect(kinds.filter((kind) => kind === 'downloading').length).toBeGreaterThan(2);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('ends the install in a signature failure in the failed scenario', async () => {
    vi.useFakeTimers();
    const { transport, pushed } = await setup('update=failed');
    await transport.invoke(COMMANDS.checkForUpdates);
    vi.advanceTimersByTime(UPDATE_CHECK_MS);
    await transport.invoke(COMMANDS.installUpdate);
    vi.advanceTimersByTime(UPDATE_TICK_MS * 10);
    expect(pushed.at(-1)?.phase).toEqual({ kind: 'failed', error: 'signature' });
  });

  it('ignores install when no update is available', async () => {
    const { transport, pushed } = await setup('update=uptodate');
    const returned = await transport.invoke<UpdateState>(COMMANDS.installUpdate);
    expect(returned.phase).toEqual({ kind: 'upToDate' });
    expect(pushed).toEqual([]);
  });

  it('mirrors autoCheckUpdates into the update state', async () => {
    const { transport, pushed } = await setup('');
    const current = await transport.invoke<Settings>(COMMANDS.getSettings);
    expect(current.autoCheckUpdates).toBe(true);
    await transport.invoke(COMMANDS.setSettings, {
      settings: { ...current, autoCheckUpdates: false },
    });
    expect(pushed.at(-1)?.autoCheck).toBe(false);
    expect((await transport.invoke<UpdateState>(COMMANDS.getUpdateState)).autoCheck).toBe(false);
  });

  it('still rejects unknown commands', async () => {
    const { transport } = await setup('');
    await expect(transport.invoke('nope')).rejects.toThrow('mock: unknown command nope');
  });
});
