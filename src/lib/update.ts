import { formatDateTime } from './format';
import { t } from './i18n';
import type { UpdateError, UpdateState } from './types';

export type UpdateTone = 'neutral' | 'accent' | 'warn' | 'critical';

export interface UpdateProgress {
  value: number | null;
  text: string;
}

export interface UpdateView {
  statusText: string | null;
  tone: UpdateTone;
  busy: boolean;
  canCheck: boolean;
  showInstall: boolean;
  version: string | null;
  notes: string | null;
  progress: UpdateProgress | null;
  hint: string | null;
  errorText: string | null;
}

export interface PostUpdateView {
  title: string;
  loginText: string | null;
  showOpenSettings: boolean;
}

const BYTES_PER_MEGABYTE = 1_000_000;
const MS_PER_MINUTE = 60_000;
const MS_PER_HOUR = 3_600_000;
const MS_PER_DAY = 86_400_000;
const NOTES_COLLAPSED_LINES = 3;
const NOTES_COLLAPSED_CHARS = 140;

const megabyteFormat = new Intl.NumberFormat('vi-VN', {
  minimumFractionDigits: 1,
  maximumFractionDigits: 1,
});

const ERROR_KEYS = {
  network: 'update.error.network',
  timeout: 'update.error.timeout',
  signature: 'update.error.signature',
  tooLarge: 'update.error.tooLarge',
  install: 'update.error.install',
  unknown: 'update.error.unknown',
} as const satisfies Record<UpdateError, string>;

function megabytes(bytes: number): string {
  return megabyteFormat.format(Math.max(0, bytes) / BYTES_PER_MEGABYTE);
}

function base(): UpdateView {
  return {
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
}

const busy = { busy: true, canCheck: false } as const;

export function downloadProgress(received: number, total: number | null): UpdateProgress {
  if (total === null || total <= 0) {
    return { value: null, text: t('update.progressUnknown', { received: megabytes(received) }) };
  }
  return {
    value: Math.min(100, Math.max(0, (received / total) * 100)),
    text: t('update.progress', { received: megabytes(received), total: megabytes(total) }),
  };
}

export function updateView(state: UpdateState): UpdateView {
  const phase = state.phase;
  switch (phase.kind) {
    case 'idle':
      return base();
    case 'checking':
      return { ...base(), ...busy, statusText: t('update.status.checking') };
    case 'upToDate':
      return { ...base(), statusText: t('update.status.upToDate') };
    case 'available': {
      const notes = phase.notes.trim();
      return {
        ...base(),
        statusText: t('update.status.available', { version: phase.version }),
        tone: 'accent',
        showInstall: true,
        version: phase.version,
        notes: notes === '' ? null : notes,
      };
    }
    case 'downloading':
      return {
        ...base(),
        ...busy,
        statusText: t('update.status.downloading'),
        tone: 'accent',
        progress: downloadProgress(phase.received, phase.total),
      };
    case 'installing':
      return {
        ...base(),
        ...busy,
        statusText: t('update.status.installing'),
        tone: 'accent',
        hint: t('update.installingHint'),
      };
    case 'failed':
      return {
        ...base(),
        statusText: t('update.status.failed'),
        tone: 'critical',
        errorText: t(ERROR_KEYS[phase.error]),
      };
    case 'blocked':
      return {
        ...base(),
        statusText: t('update.status.blocked'),
        tone: 'warn',
        hint: t('update.blockedHint'),
      };
  }
}

export function lastCheckText(lastCheckAt: number | null, now: number, timeZone?: string): string {
  if (lastCheckAt === null) {
    return t('update.lastCheck', { when: t('update.lastCheck.never') });
  }
  const elapsed = now - lastCheckAt;
  let when: string;
  if (elapsed < MS_PER_MINUTE) {
    when = t('update.lastCheck.justNow');
  } else if (elapsed < MS_PER_HOUR) {
    when = t('update.lastCheck.minutes', { count: Math.floor(elapsed / MS_PER_MINUTE) });
  } else if (elapsed < MS_PER_DAY) {
    when = t('update.lastCheck.hours', { count: Math.floor(elapsed / MS_PER_HOUR) });
  } else {
    when = formatDateTime(lastCheckAt / 1000, timeZone);
  }
  return t('update.lastCheck', { when });
}

export function postUpdateView(state: UpdateState): PostUpdateView | null {
  const post = state.postUpdate;
  if (post === null) {
    return null;
  }
  const title = t('update.post.title', { version: state.currentVersion });
  switch (post.loginItem) {
    case 'requiresApproval':
      return { title, loginText: t('update.post.loginApproval'), showOpenSettings: true };
    case 'failed':
      return { title, loginText: t('update.post.loginFailed'), showOpenSettings: false };
    case 'unchanged':
    case 'reRegistered':
      return { title, loginText: null, showOpenSettings: false };
  }
}

export function showUpdateBanner(state: UpdateState | null, installAttempted: boolean): boolean {
  if (state === null) {
    return false;
  }
  if (state.postUpdate !== null) {
    return true;
  }
  switch (state.phase.kind) {
    case 'available':
    case 'downloading':
    case 'installing':
      return true;
    case 'failed':
    case 'blocked':
      return installAttempted;
    case 'idle':
    case 'checking':
    case 'upToDate':
      return false;
  }
}

export function notesAreLong(notes: string): boolean {
  return notes.split('\n').length > NOTES_COLLAPSED_LINES || notes.length > NOTES_COLLAPSED_CHARS;
}
