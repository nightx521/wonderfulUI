<template>
  <section class="history-import" aria-labelledby="history-import-title">
    <h3 id="history-import-title">导入旧电脑数据</h3>
    <p>先把旧电脑的 WonderfulDb 文件夹和视频复制到当前电脑。仅有视频，无法恢复对局和事件信息。</p>
    <button v-if="!expanded" class="btn" type="button" @click="expanded = true">开始导入</button>
    <template v-else>
      <fieldset :disabled="busy" class="import-fields">
        <label>设备名称 <input v-model="deviceName" class="import-name" maxlength="40" placeholder="例如：旧电脑" @input="invalidate" /></label>
        <div class="import-choice">
          <button class="btn" type="button" @click="choose('database')">1. 选择旧 WonderfulDb 文件夹</button>
          <span class="import-path">{{ database?.path ?? '包含无扩展名的数字账户文件，可附带 snapshot 文件' }}</span>
        </div>
        <div class="import-choice">
          <button class="btn" type="button" @click="choose('videos')">2. 选择视频现在所在的文件夹</button>
          <span class="import-path">{{ videos?.path ?? '选择当前电脑上的目录，会查找其子文件夹' }}</span>
        </div>
        <button class="btn" type="button" :disabled="!database || !videos || !deviceName.trim()" @click="makePreview">3. 预览导入结果</button>
      </fieldset>
      <div v-if="preview" class="import-preview" aria-label="导入预览">
        <p>{{ preview.accounts }} 个账户 · 新增 {{ preview.newMatches }} 场 · 已有 {{ preview.duplicateMatches }} 场</p>
        <p>找到 {{ preview.foundVideos }} 个视频 · 缺失 {{ preview.missingVideos }} 个 · 匹配不唯一 {{ preview.ambiguousVideos }} 个</p>
        <p v-if="preview.failedAccounts" class="import-warning">{{ preview.failedAccounts }} 个账户解析失败，将跳过这些账户。</p>
        <p v-if="preview.conflictingMatches" class="import-warning">{{ preview.conflictingMatches }} 场已有对局的资料不同，将保留当前资料并补充视频。</p>
        <p v-for="warning in preview.warnings" :key="warning" class="import-warning">{{ warning }}</p>
        <p>已有对局保留当前对局资料并补充视频。同一对局只显示一次，可按来源设备筛选。缺失或匹配不唯一的视频会保留对局记录，暂时无法播放。</p>
        <button class="btn btn-primary" type="button" :disabled="busy" @click="commit">确认导入</button>
      </div>
      <p v-if="busy" role="status">{{ phase }}</p>
      <p v-if="error" role="alert" class="import-error">{{ error }}</p>
      <p v-if="success" role="status">{{ success }}</p>
      <button class="btn btn-ghost" type="button" :disabled="busy" @click="cancel">取消</button>
    </template>
  </section>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue';
import { invoke } from '../../tauri-adapter.ts';
import { useAccountStore } from '../../stores/account.ts';
import { useSettingsStore } from '../../stores/settings.ts';

interface Selection { token: string; path: string }
interface Preview {
  token: string; accounts: number; newMatches: number; duplicateMatches: number;
  conflictingMatches: number;
  foundVideos: number; missingVideos: number; ambiguousVideos: number;
  failedAccounts: number; warnings: string[];
}
const account = useAccountStore();
const settings = useSettingsStore();
const expanded = ref(false);
const busy = ref(false);
const deviceName = ref('旧电脑');
const database = ref<Selection | null>(null);
const videos = ref<Selection | null>(null);
const preview = ref<Preview | null>(null);
const error = ref('');
const success = ref('');
const phase = ref('');
function invalidate() { preview.value = null; error.value = ''; success.value = ''; }
async function work(label: string, fn: () => Promise<void>) {
  busy.value = true; phase.value = label; error.value = ''; success.value = '';
  try { await fn(); } catch (e) { error.value = String(e instanceof Error ? e.message : e); }
  finally { busy.value = false; }
}
async function choose(kind: 'database' | 'videos') {
  await work('正在选择目录…', async () => {
    const result = await invoke<Selection | null>('select_import_directory', { kind });
    if (!result) return;
    invalidate();
    if (kind === 'database') database.value = result; else videos.value = result;
  });
}
async function makePreview() {
  if (!database.value || !videos.value) return;
  preview.value = null;
  await work('正在读取旧数据并匹配视频…', async () => {
    preview.value = await invoke<Preview>('preview_history_import', {
      databaseToken: database.value!.token, videoToken: videos.value!.token, deviceName: deviceName.value.trim(),
    });
  });
}
async function commit() {
  if (!preview.value) return;
  await work('正在导入资料库…', async () => {
    await invoke('commit_history_import', { token: preview.value!.token });
    preview.value = null;
    success.value = '导入完成。可在筛选中选择来源设备。';
    try { await account.loadLibrary(); await settings.fetchLibraryStats(); }
    catch { error.value = '数据已导入，界面刷新失败，请重新打开应用。'; }
  });
}
function cancel() {
  void invoke('cancel_history_import').catch(() => {});
  database.value = null; videos.value = null; invalidate(); expanded.value = false;
}
onBeforeUnmount(() => { if (!busy.value) void invoke('cancel_history_import').catch(() => {}); });
</script>

<style scoped>
.history-import { padding: 16px; border: 1px solid var(--border-soft); border-radius: var(--radius); background: var(--surface-2); }
.history-import h3 { margin: 0 0 10px; font-size: 14px; color: var(--ink); }
.history-import p { font-size: 12px; line-height: 1.6; color: var(--ink-3); margin: 8px 0; }
.import-fields { border: 0; margin: 12px 0; padding: 0; display: flex; flex-direction: column; gap: 12px; align-items: flex-start; min-width: 0; }
.import-fields label { font-size: 12px; color: var(--ink-2); display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
.import-name { padding: 6px 8px; border-radius: 4px; background: var(--surface); border: 1px solid var(--border-soft); color: var(--ink); font: inherit; }
.import-choice { display: flex; flex-direction: column; gap: 6px; width: 100%; align-items: flex-start; }
.import-path { font-size: 12px; color: var(--ink-3); overflow-wrap: anywhere; user-select: text; }
.import-preview { margin-bottom: 12px; padding: 12px; border: 1px solid var(--border-soft); border-radius: 4px; }
.history-import .import-error { color: var(--loss); }
.history-import .import-warning { color: var(--ink-2); }
</style>
