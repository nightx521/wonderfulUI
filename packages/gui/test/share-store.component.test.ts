import { beforeEach, describe, expect, test, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock('../src/tauri-adapter.ts', () => ({
  invoke: invokeMock,
}));

import {
  FRIENDLY_SHARE_STOP_ERROR,
  useShareStore,
  type ShareServerInfo,
} from '../src/stores/share.ts';

function info(sessionId: string): ShareServerInfo {
  return {
    sessionId,
    port: 22357,
    token: `token-${sessionId}`,
    url: `http://192.168.1.42:22357/w/token-${sessionId}`,
    lanIp: '192.168.1.42',
    qrSvg: '<svg/>',
    videoName: 'clip.mp4',
    videoSize: 1024,
    startedAtUnix: 1,
  };
}

beforeEach(() => {
  setActivePinia(createPinia());
  invokeMock.mockReset();
});

describe('share store session races', () => {
  test('a replacement start waits until the preceding shutdown is acknowledged', async () => {
    let finishStop!: () => void;
    invokeMock.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === 'start_share_server') return Promise.resolve(info(String(args?.sessionId)));
      if (command === 'stop_share_server') return new Promise<void>(resolve => { finishStop = resolve; });
      return Promise.resolve(undefined);
    });
    const share = useShareStore();
    await share.start('D:\\Highlights\\first.mp4');
    const stopping = share.stop();
    const starting = share.start('D:\\Highlights\\second.mp4');
    await Promise.resolve();
    expect(invokeMock.mock.calls.filter(([command]) => command === 'start_share_server')).toHaveLength(1);
    finishStop();
    await Promise.all([stopping, starting]);
    expect(share.status).toBe('running');
    expect(invokeMock.mock.calls.filter(([command]) => command === 'start_share_server')).toHaveLength(2);
  });
  test('normal close stays open while a native download is active and force close is explicit', async () => {
    invokeMock.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === 'start_share_server') return Promise.resolve(info(String(args?.sessionId)));
      if (command === 'stop_share_server') return Promise.resolve({ stopped: args?.force === true, activeDownloads: args?.force ? 0 : 1 });
      return Promise.resolve(undefined);
    });
    const share = useShareStore();
    await share.start('D:\\Highlights\\clip.mp4');
    const sessionId = share.activeSessionId;
    expect(await share.stop(false)).toBe(false);
    expect(share.status).toBe('running');
    expect(share.activeSessionId).toBe(sessionId);
    expect(share.activeDownloads).toBe(1);
    expect(await share.stop(true)).toBe(true);
    expect(share.status).toBe('idle');
    expect(invokeMock).toHaveBeenLastCalledWith('stop_share_server', { sessionId, force: true });
  });

  test('a close error remains visible instead of reporting successful shutdown', async () => {
    invokeMock.mockImplementation((command: string, args?: Record<string, unknown>) => command === 'start_share_server'
      ? Promise.resolve(info(String(args?.sessionId))) : Promise.reject(new Error('stop failed')));
    const share = useShareStore();
    await share.start('D:\\Highlights\\clip.mp4');
    expect(await share.stop(false)).toBe(false);
    expect(share.info).not.toBeNull();
    expect(share.lastError).toBe('快传关闭失败，请重试');
  });

  test('port conflicts expose the native application name and use its ticket for release', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'start_share_server') return Promise.reject('WUI_SHARE_PORT_IN_USE|');
      if (command === 'inspect_share_port') return Promise.resolve([{ pid: 42, name: 'example.exe', ticket: 'native-ticket', reason: null }]);
      return Promise.resolve(undefined);
    });
    const share = useShareStore();
    await share.start('D:\\Highlights\\clip.mp4');
    expect(share.portOwners[0]?.name).toBe('example.exe');
    expect(await share.releasePort('native-ticket')).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith('release_share_port', { ticket: 'native-ticket' });
  });
  test('stop during start invalidates UI and cleans the late backend session', async () => {
    let resolveStart!: (value: ShareServerInfo) => void;
    let startSessionId = '';
    invokeMock.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === 'start_share_server') {
        startSessionId = String(args?.sessionId);
        return new Promise<ShareServerInfo>(resolve => { resolveStart = resolve; });
      }
      return Promise.resolve(undefined);
    });

    const share = useShareStore();
    const startPromise = share.start('D:\\Highlights\\clip.mp4');
    expect(share.status).toBe('starting');

    const stopPromise = share.stop();
    expect(share.status).toBe('idle');
    expect(share.activeSessionId).toBeNull();

    resolveStart(info(startSessionId));
    await Promise.all([startPromise, stopPromise]);

    expect(share.status).toBe('idle');
    expect(share.info).toBeNull();
    const scopedStops = invokeMock.mock.calls.filter(([command]) => command === 'stop_share_server');
    expect(scopedStops.length).toBeGreaterThanOrEqual(1);
    expect(scopedStops.every(([, args]) => args.sessionId === startSessionId)).toBe(true);
  });

  test('events from a replaced server cannot clear or update the current one', async () => {
    invokeMock.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === 'start_share_server') {
        return Promise.resolve(info(String(args?.sessionId)));
      }
      return Promise.resolve(undefined);
    });

    const share = useShareStore();
    await share.start('D:\\Highlights\\first.mp4');
    const oldSessionId = share.info!.sessionId;
    share.onActivity({ sessionId: oldSessionId, activeDownloads: 1 });
    await share.start('D:\\Highlights\\second.mp4');
    const currentSessionId = share.info!.sessionId;

    expect(currentSessionId).not.toBe(oldSessionId);
    expect(share.onStopped({ sessionId: oldSessionId, reason: 'stopped' })).toBe(false);
    expect(share.onDownloaded({
      sessionId: oldSessionId,
      count: 99,
      filename: 'old.mp4',
      sizeBytes: 99,
    })).toBe(false);
    expect(share.status).toBe('running');
    expect(share.info?.sessionId).toBe(currentSessionId);
    expect(share.downloadCount).toBe(0);
    expect(share.activeDownloads).toBe(0);
    expect(share.onActivity({ sessionId: oldSessionId, activeDownloads: 1 })).toBe(false);

    expect(share.onDownloaded({
      sessionId: currentSessionId,
      count: 1,
      filename: 'second.mp4',
      sizeBytes: 1024,
    })).toBe(true);
    expect(share.downloadCount).toBe(1);
  });

  test('keeps native paths and server details out of visible errors', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    invokeMock.mockRejectedValueOnce(
      new Error('WUI_SHARE_PORT_IN_USE|native detail D:\\Private\\clip.mp4'),
    );
    const share = useShareStore();

    await share.start('D:\\Private\\clip.mp4');
    expect(share.status).toBe('error');
    expect(share.lastError).toBe('快传端口 22357 被占用，请关闭占用该端口的程序后重试。');
    expect(share.lastError).not.toContain('Private');

    const sessionId = share.activeSessionId!;
    expect(share.onStopped({
      sessionId,
      reason: 'error',
      message: 'WUI_SHARE_FIREWALL_AUTHORIZATION_FAILED|listener detail',
    })).toBe(true);
    expect(share.lastError).toBe(FRIENDLY_SHARE_STOP_ERROR);
    expect(share.lastError).not.toContain('53124');
    consoleError.mockRestore();
  });
});
