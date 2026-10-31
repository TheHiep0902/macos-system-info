import { describe, expect, it } from 'vitest';
import fixture from '../mocks/contract-update.json';
import type { LoginItemOutcome, UpdateError, UpdateState } from './types';

interface UpdateFixture {
  idle: UpdateState;
  checking: UpdateState;
  upToDate: UpdateState;
  available: UpdateState;
  availableNoDate: UpdateState;
  downloading: UpdateState;
  downloadingUnknownTotal: UpdateState;
  installing: UpdateState;
  failed: UpdateState;
  blocked: UpdateState;
  postUpdate: UpdateState;
  failedErrors: UpdateError[];
  loginItemOutcomes: LoginItemOutcome[];
}

const LAST_CHECK = 1793437323000;

function state(phase: UpdateState['phase'], extra: Partial<UpdateState> = {}): UpdateState {
  return {
    currentVersion: '0.3.0',
    autoCheck: true,
    lastCheckAt: LAST_CHECK,
    phase,
    postUpdate: null,
    ...extra,
  };
}

const typed = {
  idle: state({ kind: 'idle' }, { lastCheckAt: null }),
  checking: state({ kind: 'checking' }, { autoCheck: false }),
  upToDate: state({ kind: 'upToDate' }),
  available: state({
    kind: 'available',
    version: '0.3.1',
    notes: 'Sửa lỗi hiển thị nhiệt độ.\nCải thiện tốc độ mở popover.',
    date: 1793430000000,
  }),
  availableNoDate: state({ kind: 'available', version: '0.4.0', notes: '', date: null }),
  downloading: state({ kind: 'downloading', received: 1048576, total: 2621440 }),
  downloadingUnknownTotal: state({ kind: 'downloading', received: 4096, total: null }),
  installing: state({ kind: 'installing' }),
  failed: state({ kind: 'failed', error: 'signature' }),
  blocked: state(
    { kind: 'blocked', reason: 'Hãy chép Pulsebar vào thư mục Applications rồi bật lại' },
    { lastCheckAt: null },
  ),
  postUpdate: state(
    { kind: 'idle' },
    { currentVersion: '0.3.1', postUpdate: { fromVersion: '0.3.0', loginItem: 'reRegistered' } },
  ),
  failedErrors: ['network', 'timeout', 'signature', 'tooLarge', 'install', 'unknown'],
  loginItemOutcomes: ['unchanged', 'reRegistered', 'requiresApproval', 'failed'],
} satisfies UpdateFixture;

describe('updater IPC contract fixture', () => {
  it('matches the typed TypeScript contract exactly', () => {
    expect(fixture).toStrictEqual(typed);
  });
});
