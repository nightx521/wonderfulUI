import { Update } from '@tauri-apps/plugin-updater';
import { invoke } from '../tauri-adapter.ts';

export type UpdateChannel = 'proxy' | 'github';
const CHANNEL_KEY = 'wui:update.channel';

export function loadUpdateChannel(storage?: Pick<Storage, 'getItem'> | null): UpdateChannel {
  try {
    const source = storage === undefined ? localStorage : storage;
    return source?.getItem(CHANNEL_KEY) === 'github' ? 'github' : 'proxy';
  } catch { return 'proxy'; }
}

export function saveUpdateChannel(channel: UpdateChannel, storage?: Pick<Storage, 'setItem'> | null): void {
  try {
    const target = storage === undefined ? localStorage : storage;
    target?.setItem(CHANNEL_KEY, channel);
  } catch { /* Retain the in-memory selection. */ }
}

export async function check(channel: UpdateChannel = 'proxy'): Promise<Update | null> {
  const metadata = await invoke<ConstructorParameters<typeof Update>[0] | null>('check_project_update', { channel });
  return metadata ? new Update(metadata) : null;
}
