// "快传" 跨设备分享 Pinia store。
//
// 状态机：
//   idle       → 没启动过 / 启动前
//   starting   → invoke 启动中（极短）
//   running    → server 跑着，info 有 URL + QR
//   error      → 上次启动失败
//
// 关闭走 stop()；server 端 3 分钟空闲会自动停（Rust 端发
// `wui://share_server_stopped` 事件），前端收到后 reset 回 idle。

import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '../tauri-adapter.ts';
import {
  canBeginShareStart,
  createShareSessionId,
  shouldCommitShareStart,
  shouldHandleShareEvent,
} from '../utils/share-start.ts';
import { clientLog } from '../utils/client-log.ts';
import {
  friendlyShareError,
} from '../utils/share-errors.ts';

export { FRIENDLY_SHARE_START_ERROR, FRIENDLY_SHARE_STOP_ERROR } from '../utils/share-errors.ts';

export interface ShareServerInfo {
  sessionId: string;
  port: number;
  token: string;
  url: string;
  lanIp: string;
  qrSvg: string;
  videoName: string;
  videoSize: number;
  /** Rust 端 server 启动时的 Unix epoch 秒。倒计时起点。 */
  startedAtUnix: number;
}

export interface ShareDownloadedEvent {
  sessionId: string;
  count: number;
  filename: string;
  sizeBytes: number;
}

export interface ShareActivityEvent { sessionId: string; activeDownloads: number }
export interface PortOwner { pid: number; name: string; ticket: string | null; reason: string | null }
interface StopOutcome { stopped: boolean; activeDownloads: number }

export type ShareStatus = 'idle' | 'starting' | 'running' | 'error';

export interface ShareStoppedEvent {
  sessionId: string;
  reason: 'stopped' | 'error' | 'idle_timeout';
  message?: string;
}

export const useShareStore = defineStore('share', () => {
  const status = ref<ShareStatus>('idle');
  const info = ref<ShareServerInfo | null>(null);
  const downloadCount = ref(0);
  const lastBytes = ref(0);
  const lastError = ref('');
  const activeSessionId = ref<string | null>(null);
  const activeDownloads = ref(0);
  const portOwners = ref<PortOwner[]>([]);
  let pendingStop: Promise<boolean> | null = null;

  async function stopBackendSession(sessionId: string): Promise<boolean> {
    const previous = pendingStop;
    const request = (async () => {
      await previous;
      try { await invoke('stop_share_server', { sessionId, force: true }); return true; }
      catch { return false; }
    })();
    pendingStop = request;
    try { return await request; }
    finally { if (pendingStop === request) pendingStop = null; }
  }

  function resetVisibleState(): void {
    info.value = null;
    status.value = 'idle';
    downloadCount.value = 0;
    lastBytes.value = 0;
    activeDownloads.value = 0;
    portOwners.value = [];
  }

  async function start(videoPath: string) {
    // Ignore double-clicks while the first invoke is in flight. Starting from
    // a running session first stops that exact old session, then begins a new
    // identity so late events cannot overwrite the replacement.
    if (!canBeginShareStart(status.value)) return;
    const previousSessionId = activeSessionId.value;
    const requestSessionId = createShareSessionId();
    activeSessionId.value = requestSessionId;
    status.value = 'starting';
    info.value = null;
    downloadCount.value = 0;
    lastBytes.value = 0;
    lastError.value = '';
    portOwners.value = [];
    activeDownloads.value = 0;
    if (previousSessionId !== null) {
      await stopBackendSession(previousSessionId);
      if (!shouldCommitShareStart(
        status.value,
        activeSessionId.value,
        requestSessionId,
      )) return;
    }
    try {
      if (pendingStop) await pendingStop;
      if (!shouldCommitShareStart(status.value, activeSessionId.value, requestSessionId)) return;
      const result = await invoke<ShareServerInfo>('start_share_server', {
        path: videoPath,
        sessionId: requestSessionId,
      });
      // stop() or a replacement start may have run during await. Do not
      // resurrect its UI, and stop only the stale backend session that this
      // result created (never whatever server is current now).
      if (!shouldCommitShareStart(
        status.value,
        activeSessionId.value,
        requestSessionId,
      )) {
        await stopBackendSession(result.sessionId);
        return;
      }
      info.value = result;
      downloadCount.value = 0;
      status.value = 'running';
    } catch (e) {
      if (!shouldCommitShareStart(
        status.value,
        activeSessionId.value,
        requestSessionId,
      )) return;
      const message = e instanceof Error ? e.message : String(e);
      clientLog('error', 'share-store', `start failed: ${message}`);
      lastError.value = friendlyShareError(e, 'start');
      info.value = null;
      status.value = 'error';
      if (String(e).includes('WUI_SHARE_PORT_IN_USE')) await inspectPort();
    }
  }

  async function stop(force = true): Promise<boolean> {
    const sessionId = activeSessionId.value;
    const previousInfo = info.value;
    if (!force && sessionId !== null) {
      try {
        const result = await invoke<StopOutcome | undefined>('stop_share_server', { sessionId, force: false });
        if (activeSessionId.value !== sessionId && activeSessionId.value !== null) return false;
        if (result && !result.stopped) { activeDownloads.value = result.activeDownloads; return false; }
      } catch (error) {
        clientLog('error', 'share-store', `stop failed: ${error}`);
        lastError.value = '快传关闭失败，请重试';
        return false;
      }
    }
    // Invalidate synchronously before awaiting IPC. An in-flight start sees
    // the mismatch and cleans up its own backend session after it returns.
    activeSessionId.value = null;
    resetVisibleState();
    if (force && sessionId !== null && !await stopBackendSession(sessionId)) {
      if (activeSessionId.value === null) {
        activeSessionId.value = sessionId;
        info.value = previousInfo;
        lastError.value = '快传关闭失败，请重试';
        status.value = 'error';
      }
      return false;
    }
    return true;
  }

  async function inspectPort() {
    const session = activeSessionId.value;
    try { const owners = await invoke<PortOwner[]>('inspect_share_port') ?? []; if (activeSessionId.value === session) portOwners.value = owners; }
    catch { if (activeSessionId.value === session) portOwners.value = []; }
  }

  async function releasePort(ticket: string): Promise<boolean> {
    try {
      await invoke('release_share_port', { ticket });
      await inspectPort();
      return true;
    } catch {
      lastError.value = '无法结束占用程序，占用信息可能已变化，请重新检查或自行退出该程序';
      await inspectPort();
      return false;
    }
  }

  function onActivity(payload: ShareActivityEvent): boolean {
    if (!shouldHandleShareEvent(activeSessionId.value, payload.sessionId)) return false;
    activeDownloads.value = payload.activeDownloads;
    return true;
  }

  /** 收到 `wui://share_downloaded` 事件时调用。 */
  function onDownloaded(payload: ShareDownloadedEvent): boolean {
    if (!shouldHandleShareEvent(activeSessionId.value, payload.sessionId)) return false;
    downloadCount.value = Math.max(downloadCount.value, payload.count);
    lastBytes.value = payload.sizeBytes;
    return true;
  }

  /** 收到 `wui://share_server_stopped` 事件时调用。 */
  function onStopped(payload: ShareStoppedEvent): boolean {
    if (!shouldHandleShareEvent(activeSessionId.value, payload.sessionId)) return false;
    activeSessionId.value = null;
    if (payload.reason === 'error') {
      if (payload.message) {
        clientLog('error', 'share-store', `server stopped: ${payload.message}`);
      }
      lastError.value = friendlyShareError(payload.message, 'stop');
      status.value = 'error';
    } else {
      status.value = 'idle';
    }
    info.value = null;
    downloadCount.value = 0;
    lastBytes.value = 0;
    activeDownloads.value = 0;
    return true;
  }

  return {
    status, info, downloadCount, lastBytes, lastError, activeSessionId, activeDownloads, portOwners,
    start, stop, onDownloaded, onStopped, onActivity, inspectPort, releasePort,
  };
});
