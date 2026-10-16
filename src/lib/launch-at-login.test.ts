import { describe, expect, it } from 'vitest';
import fixture from '../mocks/contract-settings.json';
import { launchAtLoginUi } from './launch-at-login';
import type { LaunchAtLoginStatus } from './types';

const BLOCKED_REASON = fixture.launchAtLoginBlocked.launchAtLoginReason;
const APPROVAL_HINT = 'Cần cho phép Pulsebar trong Cài đặt hệ thống › Mục đăng nhập';
const BLOCKED_FALLBACK = 'Không bật được khởi động cùng macOS lúc này';

function ui(launchAtLoginStatus: LaunchAtLoginStatus, launchAtLoginReason: string | null = null) {
  return launchAtLoginUi({ launchAtLoginStatus, launchAtLoginReason });
}

describe('launchAtLoginUi', () => {
  it.each(['enabled', 'disabled'] as const)('leaves the switch plain when %s', (status) => {
    expect(ui(status)).toEqual({ disabled: false, hint: undefined, showOpenSettings: false });
  });

  it('asks for approval and offers to open System Settings when approval is required', () => {
    expect(ui('requiresApproval')).toEqual({
      disabled: false,
      hint: APPROVAL_HINT,
      showOpenSettings: true,
    });
  });

  it('disables the switch and shows the reason from Rust as-is when blocked', () => {
    expect(ui('blocked', BLOCKED_REASON)).toEqual({
      disabled: true,
      hint: 'Hãy chép Pulsebar vào thư mục Applications rồi bật lại',
      showOpenSettings: false,
    });
  });

  it.each([null, '', '   '])(
    'falls back to a generic hint when blocked with reason %j',
    (reason) => {
      expect(ui('blocked', reason)).toEqual({
        disabled: true,
        hint: BLOCKED_FALLBACK,
        showOpenSettings: false,
      });
    },
  );

  it('ignores a stray reason outside the blocked state', () => {
    expect(ui('requiresApproval', 'x').hint).toBe(APPROVAL_HINT);
    expect(ui('enabled', 'x').hint).toBeUndefined();
  });
});
