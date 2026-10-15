import type { Settings, TitleMetrics } from './types';

export type TitleMetricKey = keyof TitleMetrics;

export const TITLE_METRIC_KEYS: readonly TitleMetricKey[] = [
  'cpu',
  'power',
  'memory',
  'temperature',
];

export interface MenuBarLocks {
  icon: boolean;
  metrics: Readonly<Record<TitleMetricKey, boolean>>;
}

export function menuBarLocks(
  settings: Pick<Settings, 'showTrayIcon' | 'titleMetrics'>,
): MenuBarLocks {
  const shown = TITLE_METRIC_KEYS.filter((key) => settings.titleMetrics[key]);
  const lastMetric = !settings.showTrayIcon && shown.length === 1 ? shown[0] : undefined;
  return {
    icon: settings.showTrayIcon && shown.length === 0,
    metrics: {
      cpu: lastMetric === 'cpu',
      power: lastMetric === 'power',
      memory: lastMetric === 'memory',
      temperature: lastMetric === 'temperature',
    },
  };
}
