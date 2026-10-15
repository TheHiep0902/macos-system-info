import { t } from './i18n';

export type LoadStatus = 'normal' | 'warn' | 'critical';

export type DisplayStatus = LoadStatus | 'unavailable';

export type ThresholdMetric = 'cpu' | 'memory' | 'disk' | 'temperature';

interface Threshold {
  warn: number;
  critical: number;
}

export const THRESHOLDS: Readonly<Record<ThresholdMetric, Threshold>> = {
  cpu: { warn: 70, critical: 90 },
  memory: { warn: 75, critical: 90 },
  disk: { warn: 80, critical: 95 },
  temperature: { warn: 80, critical: 95 },
};

export function statusFor(metric: ThresholdMetric, percent: number): DisplayStatus {
  if (!Number.isFinite(percent)) {
    return 'unavailable';
  }
  const threshold = THRESHOLDS[metric];
  if (percent >= threshold.critical) {
    return 'critical';
  }
  if (percent >= threshold.warn) {
    return 'warn';
  }
  return 'normal';
}

export function statusLabel(status: DisplayStatus): string {
  switch (status) {
    case 'normal':
      return t('status.normal');
    case 'warn':
      return t('status.warn');
    case 'critical':
      return t('status.critical');
    case 'unavailable':
      return t('status.unavailable');
  }
}
