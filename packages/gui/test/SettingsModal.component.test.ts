import { afterEach, describe, expect, test, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createTestingPinia } from '@pinia/testing';
import SettingsModal from '../src/components/settings/SettingsModal.vue';
import * as adapter from '../src/tauri-adapter.ts';

afterEach(() => vi.restoreAllMocks());

describe('derivative project attribution', () => {
  test('keeps original and derivative copyrights and opens the derivative repository', async () => {
    const invoke = vi.spyOn(adapter, 'invoke').mockResolvedValue(undefined);
    const wrapper = mount(SettingsModal, {
      global: {
        plugins: [createTestingPinia({ createSpy: vi.fn, initialState: { settings: { activeTab: 'about' } } })],
        stubs: { AccountShareChart: true, HistoryImport: true },
      },
    });
    expect(wrapper.text()).toContain('原作者 © 2026 WizisCool');
    expect(wrapper.text()).toContain('二次开发 © 2026 murainy');
    expect(wrapper.text()).toContain('GPL-3.0-or-later');
    expect(wrapper.text()).not.toContain('检查更新');
    const github = wrapper.findAll('button').find(button => button.text() === 'GitHub');
    expect(github).toBeDefined();
    await github!.trigger('click');
    expect(invoke).toHaveBeenCalledWith('open_external_url', { url: 'https://github.com/nightx521/wonderfulUI' });
    wrapper.unmount();
  });
});
