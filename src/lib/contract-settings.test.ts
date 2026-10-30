import { describe, expect, it } from 'vitest';
import fixture from '../mocks/contract-settings.json';
import type { PopoverVisibility, Settings } from './types';

type LaunchFields = Pick<Settings, 'launchAtLogin' | 'launchAtLoginStatus' | 'launchAtLoginReason'>;

interface SettingsFixture {
  settings: Settings;
  defaultSettings: Settings;
  launchAtLoginBlocked: LaunchFields;
  launchAtLoginRequiresApproval: LaunchFields;
  visibilityShown: PopoverVisibility;
  visibilityHidden: PopoverVisibility;
}

const typed = {
  settings: {
    titleFormat: 'compact',
    showTrayIcon: false,
    titleMetrics: { cpu: true, power: false, memory: true, temperature: true, network: true },
    theme: 'dark',
    reduceEffects: true,
    showNetworkDisks: true,
    memorySaver: true,
    launchAtLogin: true,
    launchAtLoginStatus: 'enabled',
    launchAtLoginReason: null,
  },
  defaultSettings: {
    titleFormat: 'default',
    showTrayIcon: true,
    titleMetrics: { cpu: true, power: true, memory: true, temperature: false, network: false },
    theme: 'system',
    reduceEffects: false,
    showNetworkDisks: false,
    memorySaver: false,
    launchAtLogin: false,
    launchAtLoginStatus: 'disabled',
    launchAtLoginReason: null,
  },
  launchAtLoginBlocked: {
    launchAtLogin: false,
    launchAtLoginStatus: 'blocked',
    launchAtLoginReason: 'Hãy chép Pulsebar vào thư mục Applications rồi bật lại',
  },
  launchAtLoginRequiresApproval: {
    launchAtLogin: true,
    launchAtLoginStatus: 'requiresApproval',
    launchAtLoginReason: null,
  },
  visibilityShown: { visible: true, view: 'settings' },
  visibilityHidden: { visible: false, view: null },
} satisfies SettingsFixture;

describe('settings and popover IPC contract fixture', () => {
  it('matches the typed TypeScript contract exactly', () => {
    expect(fixture).toStrictEqual(typed);
  });
});
