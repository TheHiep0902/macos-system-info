import { describe, expect, it } from 'vitest';
import { TITLE_METRIC_KEYS, menuBarLocks, type TitleMetricKey } from './title-metrics';
import type { TitleMetrics } from './types';

interface MenuBar {
  showTrayIcon: boolean;
  titleMetrics: TitleMetrics;
}

const NONE: TitleMetrics = {
  cpu: false,
  power: false,
  memory: false,
  temperature: false,
  network: false,
};

function only(key: TitleMetricKey): TitleMetrics {
  return { ...NONE, [key]: true };
}

function allMenuBars(): MenuBar[] {
  const states: MenuBar[] = [];
  for (let mask = 0; mask < 2 ** (TITLE_METRIC_KEYS.length + 1); mask += 1) {
    const titleMetrics = { ...NONE };
    TITLE_METRIC_KEYS.forEach((key, index) => {
      titleMetrics[key] = (mask & (1 << index)) !== 0;
    });
    states.push({ showTrayIcon: (mask & (1 << TITLE_METRIC_KEYS.length)) !== 0, titleMetrics });
  }
  return states;
}

function showsSomething(state: MenuBar): boolean {
  return state.showTrayIcon || TITLE_METRIC_KEYS.some((key) => state.titleMetrics[key]);
}

describe('menuBarLocks', () => {
  it('locks nothing with the default settings', () => {
    expect(
      menuBarLocks({
        showTrayIcon: true,
        titleMetrics: { cpu: true, power: true, memory: true, temperature: false, network: false },
      }),
    ).toEqual({ icon: false, metrics: NONE });
  });

  it('locks network when only the rate lines are shown without the icon', () => {
    expect(menuBarLocks({ showTrayIcon: false, titleMetrics: only('network') })).toEqual({
      icon: false,
      metrics: { ...NONE, network: true },
    });
  });

  it.each(TITLE_METRIC_KEYS)('locks %s when it is the only metric and the icon is off', (key) => {
    const locks = menuBarLocks({ showTrayIcon: false, titleMetrics: only(key) });
    expect(locks.icon).toBe(false);
    expect(locks.metrics).toEqual({ ...NONE, [key]: true });
  });

  it.each(TITLE_METRIC_KEYS)('lets %s be turned off while the icon is on', (key) => {
    expect(menuBarLocks({ showTrayIcon: true, titleMetrics: only(key) })).toEqual({
      icon: false,
      metrics: NONE,
    });
  });

  it('locks the icon when no metric is shown', () => {
    expect(menuBarLocks({ showTrayIcon: true, titleMetrics: NONE })).toEqual({
      icon: true,
      metrics: NONE,
    });
  });

  it('locks nothing when two metrics are shown without the icon', () => {
    expect(
      menuBarLocks({
        showTrayIcon: false,
        titleMetrics: { ...NONE, cpu: true, temperature: true },
      }),
    ).toEqual({ icon: false, metrics: NONE });
  });

  it('leaves the icon free to turn on if the menu bar is somehow empty', () => {
    expect(menuBarLocks({ showTrayIcon: false, titleMetrics: NONE })).toEqual({
      icon: false,
      metrics: NONE,
    });
  });

  it('allows exactly the toggles that keep something in the menu bar', () => {
    for (const state of allMenuBars().filter(showsSomething)) {
      const locks = menuBarLocks(state);
      const iconToggled = { ...state, showTrayIcon: !state.showTrayIcon };
      expect(locks.icon, JSON.stringify(state)).toBe(!showsSomething(iconToggled));
      for (const key of TITLE_METRIC_KEYS) {
        const toggled = {
          ...state,
          titleMetrics: { ...state.titleMetrics, [key]: !state.titleMetrics[key] },
        };
        expect(locks.metrics[key], `${key} ${JSON.stringify(state)}`).toBe(
          !showsSomething(toggled),
        );
      }
    }
  });
});
