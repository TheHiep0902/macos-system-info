import { t } from './i18n';
import type { ClusterInfo } from './types';

const LOCALE = 'vi-VN';
const NBSP = '\u00a0';
const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'] as const;
const BYTES_PER_UNIT = 1024;
const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3600;
const SECONDS_PER_DAY = 86400;

const integerFormat = new Intl.NumberFormat(LOCALE, { maximumFractionDigits: 0 });
const oneDecimalFormat = new Intl.NumberFormat(LOCALE, {
  minimumFractionDigits: 1,
  maximumFractionDigits: 1,
});

export const PLACEHOLDER = t('common.placeholder');

function isUsable(value: number | null | undefined): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function roundTo(value: number, digits: number): number {
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}

export function formatInteger(value: number | null | undefined): string {
  return isUsable(value) ? integerFormat.format(value) : PLACEHOLDER;
}

export function formatPercent(value: number | null | undefined): string {
  return isUsable(value) ? `${integerFormat.format(value)}%` : PLACEHOLDER;
}

export function formatBytes(bytes: number | null | undefined): string {
  if (!isUsable(bytes) || bytes < 0) {
    return PLACEHOLDER;
  }
  let unitIndex = 0;
  let value = bytes;
  while (value >= BYTES_PER_UNIT && unitIndex < BYTE_UNITS.length - 1) {
    value /= BYTES_PER_UNIT;
    unitIndex += 1;
  }
  if (unitIndex === 0) {
    return `${integerFormat.format(value)}${NBSP}${BYTE_UNITS[0]}`;
  }
  let rounded = value < 100 ? roundTo(value, 1) : Math.round(value);
  if (rounded >= BYTES_PER_UNIT && unitIndex < BYTE_UNITS.length - 1) {
    rounded = roundTo(rounded / BYTES_PER_UNIT, 1);
    unitIndex += 1;
  }
  const text = rounded < 100 ? oneDecimalFormat.format(rounded) : integerFormat.format(rounded);
  return `${text}${NBSP}${BYTE_UNITS[unitIndex]}`;
}

export function formatMhz(mhz: number | null | undefined): string {
  return isUsable(mhz) ? `${integerFormat.format(mhz)}${NBSP}MHz` : PLACEHOLDER;
}

export function formatWatts(watts: number | null | undefined): string {
  if (!isUsable(watts)) {
    return PLACEHOLDER;
  }
  const rounded = roundTo(Math.max(0, watts), 1);
  const text = rounded < 10 ? oneDecimalFormat.format(rounded) : integerFormat.format(rounded);
  return `${text}${NBSP}W`;
}

const BYTES_PER_MEGABYTE = 1_000_000;
const RATE_LIMIT_MB = 10_000;

export function formatRate(bytesPerSec: number | null | undefined): string {
  if (!isUsable(bytesPerSec) || bytesPerSec < 0) {
    return PLACEHOLDER;
  }
  const rounded = roundTo(bytesPerSec / BYTES_PER_MEGABYTE, 1);
  if (rounded < 100) {
    return `${oneDecimalFormat.format(rounded)}${NBSP}MB/s`;
  }
  const whole = Math.round(rounded);
  if (whole >= RATE_LIMIT_MB) {
    return `${integerFormat.format(RATE_LIMIT_MB - 1)}+${NBSP}MB/s`;
  }
  return `${integerFormat.format(whole)}${NBSP}MB/s`;
}

export function formatCelsius(celsius: number | null | undefined): string {
  return isUsable(celsius) ? `${integerFormat.format(Math.round(celsius))}${NBSP}°C` : PLACEHOLDER;
}

export function formatUptime(totalSeconds: number | null | undefined): string {
  if (!isUsable(totalSeconds) || totalSeconds < 0) {
    return PLACEHOLDER;
  }
  const seconds = Math.floor(totalSeconds);
  const days = Math.floor(seconds / SECONDS_PER_DAY);
  const hours = Math.floor((seconds % SECONDS_PER_DAY) / SECONDS_PER_HOUR);
  const minutes = Math.floor((seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
  const parts: string[] = [];
  if (days > 0) {
    parts.push(t('unit.day', { count: integerFormat.format(days) }));
  }
  if (days > 0 || hours > 0) {
    parts.push(t('unit.hour', { count: hours }));
  }
  parts.push(t('unit.minute', { count: minutes }));
  return parts.join(' ');
}

export function formatDateTime(unixSeconds: number | null | undefined, timeZone?: string): string {
  if (!isUsable(unixSeconds)) {
    return PLACEHOLDER;
  }
  const formatter = new Intl.DateTimeFormat(LOCALE, {
    dateStyle: 'short',
    timeStyle: 'short',
    timeZone,
  });
  return formatter.format(new Date(unixSeconds * 1000));
}

const SMC_SOURCE = /^smc:([A-Za-z0-9]{1,8})$/;

export function sourceLabel(source: string): string {
  const normalized = source.toLowerCase();
  if (normalized === 'ioreport') {
    return t('source.ioReport');
  }
  if (normalized === 'nominal') {
    return t('source.nominal');
  }
  if (normalized === 'macmon') {
    return t('source.macmon');
  }
  const smc = SMC_SOURCE.exec(source);
  if (smc?.[1] !== undefined) {
    return t('source.smc', { key: smc[1] });
  }
  return source;
}

export function powerScopeLabel(scope: string): string {
  if (scope === 'cpu') {
    return t('power.scope.cpu');
  }
  if (scope === 'package') {
    return t('power.scope.package');
  }
  return scope;
}

const CLUSTER_KEYS = {
  E: 'cluster.E',
  P: 'cluster.P',
  S: 'cluster.S',
  CPU: 'cluster.CPU',
} as const;

function isKnownCluster(label: string): label is keyof typeof CLUSTER_KEYS {
  return Object.hasOwn(CLUSTER_KEYS, label);
}

export function clusterName(label: string, clusters: readonly ClusterInfo[] = []): string {
  if (isKnownCluster(label)) {
    return t(CLUSTER_KEYS[label]);
  }
  return clusters.find((cluster) => cluster.label === label)?.name ?? label;
}
