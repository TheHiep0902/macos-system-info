import { describe, expect, it } from 'vitest';
import { messages, t } from './i18n';

describe('i18n messages', () => {
  it('has no empty or whitespace-only strings', () => {
    for (const [key, value] of Object.entries(messages)) {
      expect(value.trim(), key).not.toBe('');
    }
  });

  it('uses only well-formed placeholders', () => {
    for (const [key, value] of Object.entries(messages)) {
      const stripped = value.replace(/\{\w+\}/g, '');
      expect(stripped.includes('{') || stripped.includes('}'), key).toBe(false);
    }
  });
});

describe('t', () => {
  it('returns plain messages unchanged', () => {
    expect(t('memory.title')).toBe('Bộ nhớ');
  });

  it('interpolates every placeholder', () => {
    expect(t('detail.topology', { physical: 8, logical: 16 })).toBe('8 lõi vật lý / 16 luồng');
    expect(t('freq.nominal', { nominal: '2.300 MHz', max: '4.800 MHz', pct: '100%' })).toBe(
      'Cơ bản 2.300 MHz · Tối đa 4.800 MHz · Giới hạn tốc độ 100%',
    );
  });
});
