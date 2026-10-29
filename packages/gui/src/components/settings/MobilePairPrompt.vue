<template>
  <Teleport to="body">
    <div v-if="request" class="mobile-pair-backdrop" @keydown="onKeydown">
      <section
        ref="dialog"
        class="mobile-pair-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="mobile-pair-title"
        aria-describedby="mobile-pair-description"
        tabindex="-1"
        @keydown.tab="onDialogTab"
      >
        <h2 id="mobile-pair-title">允许这台设备访问回放？</h2>
        <p id="mobile-pair-description">
          确认后，此浏览器可以浏览资料库、筛选对局并播放视频。
        </p>
        <div class="mobile-pair-device">
          <strong>{{ request.name }}</strong
          ><span>{{ request.ip }}</span>
        </div>
        <p class="mobile-pair-countdown" role="timer">
          {{ seconds }} 秒后自动拒绝
        </p>
        <p v-if="mobile.error" class="mobile-pair-error" role="alert">
          {{ mobile.error }}
        </p>
        <div class="mobile-pair-actions">
          <button
            type="button"
            class="btn mobile-pair-reject"
            :disabled="mobile.busy"
            @click="decide(false)"
          >
            拒绝
          </button>
          <button
            type="button"
            class="btn btn-primary"
            :disabled="mobile.busy || seconds === 0"
            @click="decide(true)"
          >
            允许连接
          </button>
        </div>
      </section>
    </div>
  </Teleport>
</template>
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useMobileAccessStore } from "../../stores/mobile-access";
import { useDialogFocus } from "../../utils/dialog-focus";
import { ownsTopModalLayer } from "../../utils/modal-layer";
const mobile = useMobileAccessStore();
const request = computed(() => mobile.status.pending[0]);
const dialog = ref<HTMLElement | null>(null);
const now = ref(Date.now()),
  deadline = ref(0);
let tick: ReturnType<typeof setInterval> | undefined,
  polls = 0,
  expiredId = "";
const seconds = computed(() =>
  Math.max(0, Math.ceil((deadline.value - now.value) / 1000)),
);
watch(
  () => `${mobile.status.generation}:${request.value?.id}`,
  () => {
    now.value = Date.now();
    deadline.value =
      now.value + Math.min(60, request.value?.remainingSeconds ?? 60) * 1000;
    expiredId = "";
  },
  { immediate: true },
);
const { onDialogTab } = useDialogFocus(dialog, () => !!request.value, "mobile");
async function decide(allow: boolean) {
  const device = request.value;
  if (!device || mobile.busy) return;
  await mobile.decide(device.id, allow);
  await mobile.refresh();
}
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && ownsTopModalLayer("mobile")) {
    event.preventDefault();
    event.stopPropagation();
    void decide(false);
  }
}
onMounted(() => {
  void mobile.init();
  tick = setInterval(() => {
    now.value = Date.now();
    if (++polls % 3 === 0 && mobile.status.running) void mobile.refresh();
    if (
      request.value &&
      seconds.value === 0 &&
      !mobile.busy &&
      expiredId !== request.value.id
    ) {
      expiredId = request.value.id;
      void decide(false);
    }
  }, 500);
});
onUnmounted(() => {
  if (tick) clearInterval(tick);
});
</script>
<style scoped>
.mobile-pair-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1600;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgb(0 0 0 / 72%);
}
.mobile-pair-dialog {
  width: min(440px, 100%);
  padding: 26px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  outline: none;
  box-shadow: 0 20px 70px rgb(0 0 0 / 40%);
}
h2 {
  font-size: 18px;
  font-weight: var(--w-semibold);
  margin: 0 0 12px;
}
p {
  font-size: 13px;
  color: var(--ink-2);
  line-height: 1.7;
  margin: 0;
}
.mobile-pair-device {
  padding: 16px 0;
  margin: 14px 0;
  border-block: 1px solid var(--border-soft);
  display: flex;
  flex-direction: column;
  gap: 6px;
  overflow-wrap: anywhere;
}
.mobile-pair-device strong {
  font-size: 15px;
}
.mobile-pair-device span {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--ink-3);
}
.mobile-pair-countdown {
  color: var(--ink-3);
  font-variant-numeric: tabular-nums;
}
.mobile-pair-error {
  color: var(--warn);
  margin-top: 10px;
}
.mobile-pair-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 22px;
}
.mobile-pair-actions button {
  min-height: 36px;
  min-width: 90px;
}
.mobile-pair-reject {
  border: 1px solid var(--border);
  background: transparent;
}
</style>
