import { describe, expect, it } from 'vitest';
import {
  PLACEHOLDER,
  clusterName,
  formatBytes,
  formatDateTime,
  formatInteger,
  formatMhz,
  formatPercent,
  formatUptime,
  formatWatts,
  powerScopeLabel,
  sourceLabel,
} from './format';

const NBSP = '\u00a0';
const GiB = 1024 ** 3;

describe('formatBytes', () => {
  it('uses binary units labelled GB', () => {
    expect(formatBytes(GiB)).toBe(`1,0${NBSP}GB`);
    expect(formatBytes(38654705664)).toBe(`36,0${NBSP}GB`);
    expect(formatBytes(23773642752)).toBe(`22,1${NBSP}GB`);
  });

  it('drops decimals from 100 units upwards', () => {
    expect(formatBytes(994662584320)).toBe(`926${NBSP}GB`);
    expect(formatBytes(512 * 1024 ** 2)).toBe(`512${NBSP}MB`);
  });

  it('promotes to the next unit when rounding reaches 1024', () => {
    expect(formatBytes(1023.97 * 1024 ** 2)).toBe(`1,0${NBSP}GB`);
  });

  it('formats small values in bytes and rejects invalid input', () => {
    expect(formatBytes(0)).toBe(`0${NBSP}B`);
    expect(formatBytes(1023)).toBe(`1.023${NBSP}B`);
    expect(formatBytes(-1)).toBe(PLACEHOLDER);
    expect(formatBytes(Number.NaN)).toBe(PLACEHOLDER);
    expect(formatBytes(null)).toBe(PLACEHOLDER);
  });
});

describe('formatPercent', () => {
  it('rounds to whole percent', () => {
    expect(formatPercent(23.5)).toBe('24%');
    expect(formatPercent(0)).toBe('0%');
    expect(formatPercent(100)).toBe('100%');
    expect(formatPercent(Number.POSITIVE_INFINITY)).toBe(PLACEHOLDER);
  });
});

describe('formatMhz', () => {
  it('groups thousands the Vietnamese way', () => {
    expect(formatMhz(2400)).toBe(`2.400${NBSP}MHz`);
    expect(formatMhz(980)).toBe(`980${NBSP}MHz`);
    expect(formatMhz(null)).toBe(PLACEHOLDER);
  });
});

describe('formatWatts', () => {
  it('shows one decimal comma below 10 W', () => {
    expect(formatWatts(8.25)).toBe(`8,3${NBSP}W`);
    expect(formatWatts(0.04)).toBe(`0,0${NBSP}W`);
    expect(formatWatts(9.94)).toBe(`9,9${NBSP}W`);
  });

  it('shows whole watts from 10 W upwards, including values that round up to 10', () => {
    expect(formatWatts(9.96)).toBe(`10${NBSP}W`);
    expect(formatWatts(10)).toBe(`10${NBSP}W`);
    expect(formatWatts(45.6)).toBe(`46${NBSP}W`);
    expect(formatWatts(1234)).toBe(`1.234${NBSP}W`);
  });

  it('never shows negative power', () => {
    expect(formatWatts(-0.4)).toBe(`0,0${NBSP}W`);
    expect(formatWatts(undefined)).toBe(PLACEHOLDER);
  });
});

describe('formatUptime', () => {
  it('writes days, hours and minutes in Vietnamese', () => {
    expect(formatUptime(3 * 86400 + 4 * 3600 + 12 * 60 + 59)).toBe('3 ngày 4 giờ 12 phút');
  });

  it('omits zero leading units but keeps inner zeros', () => {
    expect(formatUptime(4 * 3600)).toBe('4 giờ 0 phút');
    expect(formatUptime(86400 + 5 * 60)).toBe('1 ngày 0 giờ 5 phút');
  });

  it('shows minutes only below one hour', () => {
    expect(formatUptime(59 * 60)).toBe('59 phút');
    expect(formatUptime(30)).toBe('0 phút');
    expect(formatUptime(-5)).toBe(PLACEHOLDER);
  });
});

describe('other formatters', () => {
  it('formats integers and date-times with the vi-VN locale', () => {
    expect(formatInteger(12345678)).toBe('12.345.678');
    expect(formatDateTime(1760400000, 'UTC')).toBe('00:00 14/10/25');
    expect(formatDateTime(null)).toBe(PLACEHOLDER);
  });

  it('labels data sources for people', () => {
    expect(sourceLabel('ioreport')).toBe('IOReport');
    expect(sourceLabel('ioReport')).toBe('IOReport');
    expect(sourceLabel('smc:PCPT')).toBe('SMC · PCPT');
    expect(sourceLabel('smc:PC0C')).toBe('SMC · PC0C');
    expect(sourceLabel('nominal')).toBe('Danh định');
    expect(sourceLabel('custom')).toBe('custom');
  });

  it('names CPU clusters in Vietnamese and falls back to the Rust name', () => {
    expect(clusterName('P')).toBe('Lõi hiệu năng (P)');
    expect(clusterName('E')).toBe('Lõi tiết kiệm (E)');
    expect(clusterName('X', [{ label: 'X', name: 'Extreme', coreCount: 2, maxMhz: null }])).toBe(
      'Extreme',
    );
    expect(clusterName('constructor')).toBe('constructor');
  });

  it('labels power scopes', () => {
    expect(powerScopeLabel('cpu')).toBe('CPU');
    expect(powerScopeLabel('package')).toBe('Gói CPU');
    expect(powerScopeLabel('soc')).toBe('soc');
  });
});
