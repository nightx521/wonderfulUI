<template>
  <Teleport to="body">
    <div class="record-warning-backdrop" @mousedown.self="$emit('close')">
      <section
        ref="dialog"
        class="record-warning-confirm"
        role="alertdialog"
        aria-modal="true"
        tabindex="-1"
        :style="{ left: `${position.x}px`, top: `${position.y}px` }"
        aria-labelledby="record-warning-title"
        aria-describedby="record-warning-description"
        @keydown.tab="onDialogTab"
        @keydown.esc.stop="$emit('close')"
      >
        <h3 id="record-warning-title">忽略这些异常记录？</h3>
        <p class="record-warning-account">{{ label }}</p>
        <p id="record-warning-description">
          只记住本次已跳过的异常，之后不再重复提示。正常对局继续扫描，新异常仍会提醒。原始数据和视频都会保留。
        </p>
        <p v-if="error" role="alert">{{ error }}</p>
        <div class="record-warning-actions">
          <button
            type="button"
            class="btn"
            :disabled="busy"
            @click="$emit('close')"
          >
            取消
          </button>
          <button
            type="button"
            class="btn btn-primary"
            :disabled="busy"
            @click="confirm"
          >
            {{ busy ? "保存中" : "确认忽略" }}
          </button>
        </div>
      </section>
    </div>
  </Teleport>
</template>
<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "../../tauri-adapter";
import { useAccountStore } from "../../stores/account";
import { useDialogFocus } from "../../utils/dialog-focus";
const props = defineProps<{
  openid: string;
  expected: string;
  label: string;
  position: { x: number; y: number };
}>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLElement | null>(null),
  busy = ref(false),
  error = ref("");
const account = useAccountStore();
const { onDialogTab } = useDialogFocus(dialog, () => true, "warning");
async function confirm() {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await invoke("acknowledge_record_warnings", {
      openid: props.openid,
      expected: props.expected,
    });
    await account.loadLibrary();
    emit("close");
  } catch (e) {
    const text = String(e instanceof Error ? e.message : e);
    error.value = [
      "异常记录已更新，请刷新后重试",
      "请先执行全量扫描，再确认忽略异常记录",
    ].includes(text)
      ? text
      : "保存未完成，请刷新后重试。";
  } finally {
    busy.value = false;
  }
}
</script>
<style scoped>
.record-warning-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1550;
  background: transparent;
}
.record-warning-confirm {
  position: absolute;
  width: 310px;
  max-width: calc(100vw - 32px);
  padding: 18px;
  background: var(--surface-3);
  color: var(--ink);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  box-shadow: 0 10px 35px rgb(0 0 0 / 40%);
  outline: none;
}
h3 {
  margin: 0 0 8px;
  font-size: 14px;
}
p {
  font-size: 12px;
  line-height: 1.7;
  color: var(--ink-2);
  margin: 8px 0;
}
.record-warning-account {
  color: var(--ink);
  overflow-wrap: anywhere;
}
.record-warning-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}
.record-warning-actions button {
  min-height: 32px;
}
[role="alert"] {
  color: var(--warn);
}
</style>
