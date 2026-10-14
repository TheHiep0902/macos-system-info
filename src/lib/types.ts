export type Platform = 'appleSilicon' | 'intel';

export type FreqSource = 'ioReport' | 'nominal';

export type MetricState<T> =
  ({ state: 'ok' } & T) | { state: 'unavailable'; reason: string } | { state: 'loading' };

export interface ClusterInfo {
  label: string;
  name: string;
  coreCount: number;
  maxMhz: number | null;
}

export interface StaticInfo {
  platform: Platform;
  arch: string;
  translated: boolean;
  cpuBrand: string;
  physicalCores: number;
  logicalCores: number;
  clusters: ClusterInfo[];
  topologyAssumed: boolean;
  nominalMhz: number | null;
  maxMhz: number | null;
  totalMemoryBytes: number;
  osVersion: string;
  modelIdentifier: string;
  bootTimeUnix: number;
  powerSourceKind: string | null;
  freqSourceKind: string | null;
}

export interface CoreSample {
  id: number;
  cluster: string;
  usagePct: number;
  freqMhz: number | null;
  activeRatio: number | null;
}

export interface ClusterSample {
  label: string;
  freqMhz: number | null;
  usagePct: number;
}

export interface FrequencySample {
  source: FreqSource;
  avgMhz: number | null;
  minMhz: number | null;
  maxMhz: number | null;
  perCoreAvailable: boolean;
  speedLimitPct: number | null;
}

export interface CpuSample {
  totalUsagePct: number;
  frequency: MetricState<FrequencySample>;
  clusters: ClusterSample[];
  cores: CoreSample[];
}

export interface PowerSample {
  cpuWatts: number;
  smoothedCpuWatts: number;
  source: string;
  scope: string;
}

export interface MemorySample {
  usedBytes: number;
  totalBytes: number;
  usedPct: number;
  appBytes: number;
  wiredBytes: number;
  compressedBytes: number;
  cachedBytes: number;
  swapUsedBytes: number;
}

export interface DiskSample {
  id: string;
  name: string;
  mountPoint: string;
  fsType: string;
  isBoot: boolean;
  isRemovable: boolean;
  isNetwork: boolean;
  totalBytes: number;
  availableBytes: number;
  availableImportantBytes: number | null;
  usedBytes: number;
  usedPct: number;
}

export interface NetworkSample {
  wifiInterface: string | null;
  wifiIpv4: string | null;
  wifiIpv6: string | null;
  isWifiConnected: boolean;
  primaryInterface: string | null;
}

export interface Snapshot {
  seq: number;
  timestampMs: number;
  cpu: CpuSample;
  power: MetricState<PowerSample>;
  memory: MemorySample;
  disks: DiskSample[];
  network: NetworkSample;
  uptimeSecs: number;
}

export type TitleFormat = 'default' | 'compact';

export type ThemePreference = 'system' | 'light' | 'dark';

export interface Settings {
  titleFormat: TitleFormat;
  showTrayIcon: boolean;
  theme: ThemePreference;
  reduceEffects: boolean;
  showNetworkDisks: boolean;
  memorySaver: boolean;
  launchAtLogin: boolean;
}

export type PopoverView = 'overview' | 'cpu-detail' | 'settings';

export type RequestedView = 'overview' | 'settings';

export interface PopoverVisibility {
  visible: boolean;
  view: RequestedView | null;
}
