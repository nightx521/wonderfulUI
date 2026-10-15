import { beforeEach, describe, expect, test, vi } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { createTestingPinia } from '@pinia/testing';
import HistoryImport from '../src/components/settings/HistoryImport.vue';
import { invoke } from '../src/tauri-adapter.ts';
import { useAccountStore } from '../src/stores/account.ts';

vi.mock('../src/tauri-adapter.ts', () => ({ invoke: vi.fn() }));
const mockedInvoke = vi.mocked(invoke);
function render() { return mount(HistoryImport, { global: { plugins: [createTestingPinia({ createSpy: vi.fn })] } }); }
async function click(wrapper: ReturnType<typeof render>, text: string) {
  await wrapper.findAll('button').find(b => b.text() === text)!.trigger('click'); await flushPromises();
}
beforeEach(() => {
  mockedInvoke.mockReset();
  mockedInvoke.mockImplementation(async (cmd, args) => {
    if (cmd === 'select_import_directory') return { token: args?.kind, path: 'fixture directory' } as never;
    if (cmd === 'preview_history_import') return { token: 'preview-token', accounts: 1, newMatches: 2, duplicateMatches: 1, foundVideos: 2, missingVideos: 1, ambiguousVideos: 1, failedAccounts: 0, warnings: [] } as never;
    return undefined as never;
  });
});
describe('HistoryImport', () => {
  test('requires both directories, previews first, and commits only after confirmation', async () => {
    const wrapper = render();
    await click(wrapper, '开始导入');
    expect(wrapper.findAll('button').find(b => b.text() === '3. 预览导入结果')!.attributes('disabled')).toBeDefined();
    await click(wrapper, '1. 选择旧 WonderfulDb 文件夹');
    await click(wrapper, '2. 选择视频现在所在的文件夹');
    await click(wrapper, '3. 预览导入结果');
    expect(wrapper.text()).toContain('匹配不唯一 1 个');
    expect(mockedInvoke.mock.calls.some(([cmd]) => cmd === 'commit_history_import')).toBe(false);
    await click(wrapper, '确认导入');
    expect(mockedInvoke).toHaveBeenCalledWith('commit_history_import', { token: 'preview-token' });
    expect(useAccountStore().loadLibrary).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain('导入完成'); wrapper.unmount();
  });
  test('changing the device name invalidates the preview and cancel writes nothing', async () => {
    const wrapper = render(); await click(wrapper, '开始导入');
    await click(wrapper, '1. 选择旧 WonderfulDb 文件夹'); await click(wrapper, '2. 选择视频现在所在的文件夹'); await click(wrapper, '3. 预览导入结果');
    await wrapper.get('input').setValue('另一台电脑');
    expect(wrapper.find('[aria-label="导入预览"]').exists()).toBe(false);
    await click(wrapper, '取消');
    expect(mockedInvoke).toHaveBeenCalledWith('cancel_history_import');
    expect(mockedInvoke.mock.calls.some(([cmd]) => cmd === 'commit_history_import')).toBe(false);
    wrapper.unmount();
  });
  test('reports read errors without offering confirmation', async () => {
    const wrapper = render(); await click(wrapper, '开始导入');
    mockedInvoke.mockRejectedValueOnce(new Error('没有账户数据'));
    await click(wrapper, '1. 选择旧 WonderfulDb 文件夹');
    expect(wrapper.get('[role="alert"]').text()).toBe('没有账户数据');
    expect(wrapper.text()).not.toContain('确认导入'); wrapper.unmount();
  });
});
