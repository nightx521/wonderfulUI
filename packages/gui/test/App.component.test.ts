import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { defineComponent, h } from 'vue';
import { flushPromises, shallowMount } from '@vue/test-utils';
import { createTestingPinia } from '@pinia/testing';
import { setActivePinia } from 'pinia';
import * as tauriAdapter from '../src/tauri-adapter.ts';
import { useAccountStore } from '../src/stores/account.ts';
import { useUiStore } from '../src/stores/ui.ts';
import { useUpdateStore } from '../src/stores/update.ts';
import App from '../src/App.vue';

const BootOverlayStub = defineComponent({
  name: 'BootOverlay',
  setup(_, { expose }) {
    expose({ start: vi.fn(), complete: vi.fn() });
    return () => h('div', { class: 'boot-overlay-stub' });
  },
});

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('App startup refresh lifecycle', () => {
  test('reports skipped records after a partial startup refresh without claiming the source failed', async () => {
    let terminalHandler!: (event: { payload: Record<string, unknown> }) => void;
    vi.spyOn(tauriAdapter, 'listen').mockImplementation(async (_, handler) => {
      terminalHandler = handler as typeof terminalHandler;
      return () => {};
    });
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    const pinia = createTestingPinia({ createSpy: vi.fn });
    setActivePinia(pinia);
    const account = useAccountStore();
    const ui = useUiStore();
    account.totalErrors = 1;
    account.accounts = [{ openid: 'partial', path: '', matchCount: 12,
      error: '{"skipped_records":1}', errorCode: 'partial_records' }];
    vi.mocked(account.probeAclos).mockResolvedValue({ dir: '', dirExists: true, hasAccounts: true });
    vi.mocked(account.scanShell).mockResolvedValue(undefined);
    vi.mocked(account.loadLibrary).mockResolvedValue(undefined);
    vi.mocked(account.cacheAssets).mockResolvedValue(undefined);
    const wrapper = shallowMount(App, { global: { plugins: [pinia],
      stubs: { BootOverlay: BootOverlayStub, RouterView: true, Teleport: true } } });
    await flushPromises();
    terminalHandler({ payload: { status: 'degraded', error: 'partial records' } });
    await flushPromises();
    expect(ui.showToast).toHaveBeenCalledWith('正常记录已更新，部分记录已跳过，请查看账户提示', 'error');
    expect(ui.showToast).not.toHaveBeenCalledWith('源数据刷新失败，当前显示可用的本地资料库', 'error');
    wrapper.unmount();
  });

  test.each(['finished', 'degraded'] as const)('keeps the listener after timeout and guarded-reloads on late %s', async status => {
    let terminalHandler!: (event: { payload: Record<string, unknown> }) => void;
    const unlisten = vi.fn();
    vi.spyOn(tauriAdapter, 'listen').mockImplementation(async (event, handler) => {
      expect(event).toBe('wui://startup_refresh_finished');
      terminalHandler = handler as typeof terminalHandler;
      return unlisten;
    });
    vi.spyOn(console, 'info').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});

    const pinia = createTestingPinia({ createSpy: vi.fn });
    setActivePinia(pinia);
    const account = useAccountStore();
    const ui = useUiStore();
    vi.mocked(account.probeAclos).mockResolvedValue({
      dir: 'D:\\WonderfulDb',
      dirExists: true,
      hasAccounts: true,
    });
    vi.mocked(account.scanShell).mockResolvedValue(undefined);
    vi.mocked(account.loadLibrary).mockResolvedValue(undefined);
    vi.mocked(account.loadLibraryIfCurrent).mockResolvedValue(true);
    vi.mocked(account.cacheAssets).mockResolvedValue(undefined);

    const wrapper = shallowMount(App, {
      global: {
        plugins: [pinia],
        stubs: {
          BootOverlay: BootOverlayStub,
          RouterView: true,
          Teleport: true,
        },
      },
    });
    await flushPromises();
    expect(account.scanShell).toHaveBeenCalledTimes(1);
    expect(unlisten).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(30_000);
    await flushPromises();
    expect(wrapper.find('.app').exists()).toBe(true);
    expect(useUpdateStore().debugAvailable).toHaveBeenCalledWith({ silent: true });
    expect(unlisten).not.toHaveBeenCalled();

    if (status === 'degraded') {
      account.totalErrors = 1;
      account.accounts = [{ openid: 'partial', path: '', matchCount: 12,
        error: '{"skipped_records":1}', errorCode: 'partial_records' }];
    }
    terminalHandler({ payload: { status } });
    await flushPromises();
    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(account.loadLibraryIfCurrent).toHaveBeenCalledWith(account.libraryRevision);
    expect(ui.showToast).toHaveBeenCalledWith(status === 'finished'
      ? '资料库已在后台刷新完成' : '正常记录已更新，部分记录已跳过，请查看账户提示',
      status === 'finished' ? 'ok' : 'error');
    wrapper.unmount();
  });

  test('settles the frontend wait and stops boot work when App unmounts', async () => {
    const unlisten = vi.fn();
    vi.spyOn(tauriAdapter, 'listen').mockResolvedValue(unlisten);
    vi.spyOn(console, 'info').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});

    const pinia = createTestingPinia({ createSpy: vi.fn });
    setActivePinia(pinia);
    const account = useAccountStore();
    const ui = useUiStore();
    vi.mocked(account.probeAclos).mockResolvedValue({
      dir: 'D:\\WonderfulDb',
      dirExists: true,
      hasAccounts: true,
    });
    vi.mocked(account.scanShell).mockResolvedValue(undefined);
    vi.mocked(account.loadLibrary).mockResolvedValue(undefined);
    vi.mocked(account.cacheAssets).mockResolvedValue(undefined);

    const wrapper = shallowMount(App, {
      global: {
        plugins: [pinia],
        stubs: {
          BootOverlay: BootOverlayStub,
          RouterView: true,
          Teleport: true,
        },
      },
    });
    await flushPromises();
    expect(account.loadLibrary).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBeGreaterThan(0);

    wrapper.unmount();
    await flushPromises();

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
    expect(account.loadLibrary).toHaveBeenCalledTimes(1);
    expect(account.cacheAssets).not.toHaveBeenCalled();
    expect(ui.showToast).not.toHaveBeenCalled();
  });
});
