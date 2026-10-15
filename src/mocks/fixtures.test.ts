import { afterEach, describe, expect, it, vi } from 'vitest';
import { COMMANDS, EVENTS } from '../lib/ipc';
import type { Snapshot, StaticInfo } from '../lib/types';
import appleSnapshot from './snapshot-apple-silicon.json';
import intelSnapshot from './snapshot-intel.json';
import appleStatic from './static-apple-silicon.json';
import intelStatic from './static-intel.json';
import { createMockTransport, parseScenario } from './transport';

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

  it('include a boot volume, a removable volume and a Wi-Fi connection', () => {
    for (const snapshot of [typedFixtures.appleSnapshot, typedFixtures.intelSnapshot]) {
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
        'platform=intel&theme=dark&view=cpu-detail&wifi=off&power=unavailable&temperature=unavailable&state=loading&rosetta=on',
      ),
    );
    expect(scenario).toEqual({
      platform: 'intel',
      theme: 'dark',
      view: 'cpu-detail',
      wifi: false,
      powerUnavailable: true,
      temperatureUnavailable: true,
      loading: true,
      rosetta: true,
    });
  });

  it('falls back to defaults for unknown values', () => {
    expect(
      parseScenario(new URLSearchParams('platform=arm&theme=blue&view=../x&temperature=off')),
    ).toEqual({
      platform: 'apple',
      theme: null,
      view: 'overview',
      wifi: true,
      powerUnavailable: false,
      temperatureUnavailable: false,
      loading: false,
      rosetta: false,
    });
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
