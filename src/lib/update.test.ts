import { describe, expect, it } from 'vitest';
import fixture from '../mocks/contract-update.json';
import { t } from './i18n';
import {
  downloadProgress,
  lastCheckText,
  notesAreLong,
  postUpdateView,
  showUpdateBanner,
  updateView,
} from './update';
import type { LoginItemOutcome, UpdateError, UpdatePhase, UpdateState } from './types';

type StateName =
  | 'idle'
  | 'checking'
  | 'upToDate'
  | 'available'
  | 'availableNoDate'
  | 'downloading'
  | 'downloadingUnknownTotal'
  | 'installing'
  | 'failed'
  | 'blocked'
  | 'postUpdate';

const contract = fixture as unknown as Record<StateName, UpdateState> & {
  failedErrors: UpdateError[];
  loginItemOutcomes: LoginItemOutcome[];
};

function withPhase(phase: UpdatePhase): UpdateState {
  return { ...contract.idle, phase };
}

const IDLE_VIEW = {
  statusText: null,
  tone: 'neutral',
  busy: false,
  canCheck: true,
  showInstall: false,
  version: null,
  notes: null,
  progress: null,
  hint: null,
  errorText: null,
};

describe('updateView', () => {
  it('shows nothing extra while idle', () => {
    expect(updateView(contract.idle)).toEqual(IDLE_VIEW);
  });

  it('disables checking and installing while checking', () => {
    expect(updateView(contract.checking)).toEqual({
      ...IDLE_VIEW,
      busy: true,
      canCheck: false,
      statusText: 'Đang kiểm tra cập nhật…',
    });
  });

  it('confirms the current version is the latest', () => {
    expect(updateView(contract.upToDate)).toEqual({
      ...IDLE_VIEW,
      statusText: 'Bạn đang dùng bản mới nhất',
    });
  });

  it('offers the available version with its notes', () => {
    expect(updateView(contract.available)).toEqual({
      ...IDLE_VIEW,
      statusText: 'Có bản mới 0.3.1',
      tone: 'accent',
      showInstall: true,
      version: '0.3.1',
      notes: 'Sửa lỗi hiển thị nhiệt độ.\nCải thiện tốc độ mở popover.',
    });
  });

  it('drops blank notes', () => {
    expect(updateView(contract.availableNoDate).notes).toBeNull();
    expect(
      updateView(withPhase({ kind: 'available', version: '1.0.0', notes: ' \n ', date: null }))
        .notes,
    ).toBeNull();
  });

  it('shows the download percentage and decimal megabytes', () => {
    expect(updateView(contract.downloading)).toEqual({
      ...IDLE_VIEW,
      busy: true,
      canCheck: false,
      statusText: 'Đang tải bản cập nhật…',
      tone: 'accent',
      progress: { value: 40, text: '1,0 / 2,6 MB' },
    });
  });

  it('shows an indeterminate progress text when the size is unknown', () => {
    expect(updateView(contract.downloadingUnknownTotal).progress).toEqual({
      value: null,
      text: 'Đã tải 0,0 MB',
    });
  });

  it('warns about the administrator password while installing', () => {
    expect(updateView(contract.installing)).toEqual({
      ...IDLE_VIEW,
      busy: true,
      canCheck: false,
      statusText: 'Đang cài bản cập nhật…',
      tone: 'accent',
      hint: 'macOS có thể hỏi mật khẩu quản trị viên để hoàn tất.',
    });
  });

  it.each([
    ['network', 'Không kết nối được máy chủ cập nhật. Hãy kiểm tra kết nối mạng rồi thử lại.'],
    ['timeout', 'Máy chủ cập nhật phản hồi quá lâu. Hãy kiểm tra kết nối mạng rồi thử lại.'],
    [
      'signature',
      'Bản tải về không có chữ ký hợp lệ nên Pulsebar không cài. Bản hiện tại vẫn giữ nguyên.',
    ],
    [
      'tooLarge',
      'Bản tải về lớn bất thường nên Pulsebar đã dừng tải. Bản hiện tại vẫn giữ nguyên.',
    ],
    ['install', 'Không cài được bản cập nhật. Bản hiện tại vẫn giữ nguyên.'],
    ['unknown', 'Đã có lỗi khi cập nhật. Hãy thử lại sau.'],
  ] as const)('explains the %s failure and allows a new check', (error, text) => {
    expect(updateView(withPhase({ kind: 'failed', error }))).toEqual({
      ...IDLE_VIEW,
      statusText: 'Cập nhật không thành công',
      tone: 'critical',
      errorText: text,
    });
  });

  it('has a message for every failure kind in the contract', () => {
    for (const error of contract.failedErrors) {
      expect(updateView(withPhase({ kind: 'failed', error })).errorText).toBeTruthy();
    }
  });

  it('explains how to fix a blocked location and never offers install', () => {
    expect(updateView(contract.blocked)).toEqual({
      ...IDLE_VIEW,
      statusText: 'Không cập nhật được từ vị trí hiện tại của Pulsebar',
      tone: 'warn',
      hint: 'Hãy chép Pulsebar vào thư mục Applications, mở lại từ đó rồi cập nhật.',
    });
  });

  it('uses the same hint whatever reason Rust reports', () => {
    expect(updateView(withPhase({ kind: 'blocked', reason: '  ' })).hint).toBe(
      'Hãy chép Pulsebar vào thư mục Applications, mở lại từ đó rồi cập nhật.',
    );
  });
});

describe('downloadProgress', () => {
  it('treats a zero total as unknown', () => {
    expect(downloadProgress(2_500_000, 0)).toEqual({ value: null, text: 'Đã tải 2,5 MB' });
  });

  it('clamps the percentage to 0–100', () => {
    expect(downloadProgress(3_000_000, 2_000_000).value).toBe(100);
    expect(downloadProgress(-5, 2_000_000)).toEqual({ value: 0, text: '0,0 / 2,0 MB' });
  });
});

describe('lastCheckText', () => {
  const at = 1_793_437_323_000;

  it('says it never checked', () => {
    expect(lastCheckText(null, at)).toBe('Lần kiểm tra gần nhất: Chưa kiểm tra lần nào');
  });

  it.each([
    [0, 'vừa xong'],
    [-5_000, 'vừa xong'],
    [59_999, 'vừa xong'],
    [60_000, '1 phút trước'],
    [3_599_999, '59 phút trước'],
    [3_600_000, '1 giờ trước'],
    [86_399_999, '23 giờ trước'],
  ])('describes an elapsed %i ms as %s', (elapsed, when) => {
    expect(lastCheckText(at, at + elapsed)).toBe(`Lần kiểm tra gần nhất: ${when}`);
  });

  it('shows the absolute time after a day', () => {
    expect(lastCheckText(at, at + 86_400_000, 'UTC')).toBe('Lần kiểm tra gần nhất: 09:02 31/10/26');
  });
});

describe('postUpdateView', () => {
  it('is absent without a post-update notice', () => {
    expect(postUpdateView(contract.idle)).toBeNull();
  });

  it.each([
    ['unchanged', null, false],
    ['reRegistered', null, false],
    [
      'requiresApproval',
      'Hãy cho phép lại Pulsebar trong Cài đặt hệ thống › Mục đăng nhập để tiếp tục khởi động cùng macOS.',
      true,
    ],
    [
      'failed',
      'Không bật lại được khởi động cùng macOS. Hãy bật lại trong Cài đặt của Pulsebar.',
      false,
    ],
  ] as const)('reports the %s login item outcome', (loginItem, loginText, showOpenSettings) => {
    const state: UpdateState = {
      ...contract.postUpdate,
      postUpdate: { fromVersion: '0.3.0', loginItem },
    };
    expect(postUpdateView(state)).toEqual({
      title: 'Đã cập nhật lên 0.3.1',
      loginText,
      showOpenSettings,
    });
  });

  it('covers every login item outcome in the contract', () => {
    for (const loginItem of contract.loginItemOutcomes) {
      const state: UpdateState = {
        ...contract.postUpdate,
        postUpdate: { fromVersion: '0.3.0', loginItem },
      };
      expect(postUpdateView(state)?.title).toBe(t('update.post.title', { version: '0.3.1' }));
    }
  });
});

describe('showUpdateBanner', () => {
  it('hides without a state', () => {
    expect(showUpdateBanner(null, true)).toBe(false);
  });

  it('shows the post-update notice', () => {
    expect(showUpdateBanner(contract.postUpdate, false)).toBe(true);
  });

  it.each([
    ['available', true],
    ['downloading', true],
    ['installing', true],
    ['idle', false],
    ['checking', false],
    ['upToDate', false],
    ['blocked', false],
  ] as const)('for %s returns %s', (name, shown) => {
    expect(showUpdateBanner(contract[name], false)).toBe(shown);
  });

  it('shows a blocked location only after an install attempt', () => {
    expect(showUpdateBanner(contract.blocked, false)).toBe(false);
    expect(showUpdateBanner(contract.blocked, true)).toBe(true);
  });

  it('shows a failure only after an install attempt', () => {
    expect(showUpdateBanner(contract.failed, false)).toBe(false);
    expect(showUpdateBanner(contract.failed, true)).toBe(true);
  });
});

describe('notesAreLong', () => {
  it.each([
    ['', false],
    ['a\nb\nc', false],
    ['a\nb\nc\nd', true],
    ['x'.repeat(140), false],
    ['x'.repeat(141), true],
  ])('for %j returns %s', (notes, long) => {
    expect(notesAreLong(notes)).toBe(long);
  });
});
