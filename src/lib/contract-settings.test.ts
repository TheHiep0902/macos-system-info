import { describe, expect, it } from 'vitest';
import fixture from '../mocks/contract-settings.json';
import type { PopoverVisibility, Settings } from './types';

interface SettingsFixture {
  settings: Settings;
  defaultSettings: Settings;
  visibilityShown: PopoverVisibility;
  visibilityHidden: PopoverVisibility;
}

const typed = {
  settings: {
    titleFormat: 'compact',
    showTrayIcon: false,
    theme: 'dark',
    reduceEffects: true,
    showNetworkDisks: true,
    memorySaver: true,
    launchAtLogin: true,
  },
  defaultSettings: {
    titleFormat: 'default',
    showTrayIcon: true,
    theme: 'system',
    reduceEffects: false,
    showNetworkDisks: false,
    memorySaver: false,
    launchAtLogin: false,
  },
  visibilityShown: { visible: true, view: 'settings' },
  visibilityHidden: { visible: false, view: null },
} satisfies SettingsFixture;

describe('settings and popover IPC contract fixture', () => {
  it('matches the typed TypeScript contract exactly', () => {
    expect(fixture).toStrictEqual(typed);
  });
});
