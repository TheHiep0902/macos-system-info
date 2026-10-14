import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { PopoverView } from './types';

export interface Transport {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

export interface TransportSetup {
  transport: Transport;
  initialView: PopoverView;
}

export const tauriTransport: Transport = {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    return invoke<T>(cmd, args);
  },
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    return listen<T>(event, (message) => handler(message.payload));
  },
};

export async function createTransport(): Promise<TransportSetup> {
  if (import.meta.env.MODE === 'mock') {
    const mock = await import('../mocks/transport');
    return mock.createMockTransport(new URLSearchParams(window.location.search));
  }
  return { transport: tauriTransport, initialView: 'overview' };
}
