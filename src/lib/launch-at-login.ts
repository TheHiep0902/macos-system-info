import { t } from './i18n';
import type { Settings } from './types';

export interface LaunchAtLoginUi {
  disabled: boolean;
  hint: string | undefined;
  showOpenSettings: boolean;
}

export function launchAtLoginUi(
  settings: Pick<Settings, 'launchAtLoginStatus' | 'launchAtLoginReason'>,
): LaunchAtLoginUi {
  switch (settings.launchAtLoginStatus) {
    case 'blocked': {
      const reason = settings.launchAtLoginReason;
      return {
        disabled: true,
        hint:
          reason !== null && reason.trim() !== '' ? reason : t('settings.launchAtLogin.blocked'),
        showOpenSettings: false,
      };
    }
    case 'requiresApproval':
      return {
        disabled: false,
        hint: t('settings.launchAtLogin.approval'),
        showOpenSettings: true,
      };
    case 'enabled':
    case 'disabled':
      return { disabled: false, hint: undefined, showOpenSettings: false };
  }
}
