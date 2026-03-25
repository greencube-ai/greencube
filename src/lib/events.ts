import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { AuditEntry } from './types';

export function onActivityUpdate(
  callback: (entry: AuditEntry) => void
): Promise<UnlistenFn> {
  return listen<AuditEntry>('activity-update', (event) => {
    callback(event.payload);
  });
}

export function onAgentStatusChange(
  callback: (data: { id: string; status: string }) => void
): Promise<UnlistenFn> {
  return listen<{ id: string; status: string }>(
    'agent-status-change',
    (event) => {
      callback(event.payload);
    }
  );
}

export function onToast(
  callback: (data: { type: string; message: string }) => void
): Promise<UnlistenFn> {
  return listen<{ type: string; message: string }>('toast', (event) => {
    callback(event.payload);
  });
}
