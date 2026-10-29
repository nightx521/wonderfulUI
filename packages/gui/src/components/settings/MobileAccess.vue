<template>
  <section
    class="settings-section mobile-settings"
    aria-labelledby="mobile-access-title"
  >
    <div class="settings-section-head">
      <h3 id="mobile-access-title">手机访问</h3>
      <span class="settings-section-sub">同一局域网</span>
    </div>
    <div class="settings-row mobile-toggle-row">
      <div>
        <label for="mobile-access-switch" class="mobile-toggle-label"
          >允许手机浏览回放</label
        >
        <p class="settings-row-sub">
          手机扫码，经电脑确认后可筛选和观看资料库中的回放。
        </p>
      </div>
      <input
        id="mobile-access-switch"
        type="checkbox"
        role="switch"
        :checked="mobile.status.running"
        :aria-checked="mobile.status.running"
        :disabled="mobile.busy"
        :aria-busy="mobile.busy"
        @change="onToggle"
      />
    </div>
    <div class="settings-row mobile-toggle-row mobile-remember-row">
      <div>
        <label for="mobile-remember-switch" class="mobile-toggle-label"
          >记住已授权设备</label
        >
        <p class="settings-row-sub">
          同一浏览器可免配对重连；关闭此项会清除已保存的授权。
        </p>
      </div>
      <input
        id="mobile-remember-switch"
        type="checkbox"
        role="switch"
        :checked="!!mobile.status.rememberDevices"
        :aria-checked="!!mobile.status.rememberDevices"
        :disabled="mobile.busy"
        @change="onRememberToggle"
      />
    </div>
    <p v-if="mobile.busy" class="settings-row-sub" role="status">
      正在处理，请稍候；首次开启可能需要 Windows 防火墙授权。
    </p>
    <p v-if="mobile.error || copyError" class="mobile-error" role="alert">
      {{ mobile.error || copyError }}
    </p>
    <p v-if="mobile.status.restoreError" class="mobile-error" role="alert">
      手机访问未能自动恢复，请检查局域网后重新开启。
    </p>
    <template v-if="mobile.status.running">
      <div class="mobile-entry">
        <div
          class="mobile-qr"
          role="img"
          aria-label="手机访问配对二维码"
          v-html="mobile.status.qrSvg"
        />
        <div class="mobile-entry-copy">
          <h4>扫码，在手机上打开</h4>
          <p>
            与电脑连接同一局域网，用手机浏览器打开链接。连接请求会在下方出现。
          </p>
          <label class="mobile-link-label" for="mobile-entry-url"
            >手机访问链接</label
          ><input
            id="mobile-entry-url"
            type="text"
            readonly
            :value="mobile.status.url ?? ''"
            @focus="($event.target as HTMLInputElement).select()"
          />
          <div class="mobile-actions">
            <button class="btn settings-action" type="button" @click="copyLink">
              {{ copied ? "已复制" : "复制链接" }}</button
            ><button
              class="btn settings-action"
              type="button"
              :disabled="mobile.busy"
              @click="mobile.rotate"
            >
              刷新二维码
            </button>
          </div>
        </div>
      </div>
      <section class="mobile-devices" aria-labelledby="mobile-pending-title">
        <h4 id="mobile-pending-title">
          等待确认 <span>{{ mobile.status.pending.length }}</span>
        </h4>
        <p v-if="!mobile.status.pending.length" class="settings-row-sub">
          暂无连接请求。
        </p>
        <div
          v-for="device in mobile.status.pending"
          :key="device.id"
          class="mobile-device"
        >
          <div>
            <strong>{{ device.name }}</strong
            ><span>{{ device.ip }}</span>
          </div>
          <div class="mobile-actions">
            <button
              class="btn settings-action"
              type="button"
              :disabled="mobile.busy"
              @click="mobile.decide(device.id, false)"
            >
              拒绝</button
            ><button
              class="btn btn-primary"
              type="button"
              :disabled="mobile.busy"
              @click="mobile.decide(device.id, true)"
            >
              允许连接
            </button>
          </div>
        </div>
      </section>
    </template>
    <section
      v-if="mobile.status.running || mobile.status.devices.length"
      class="mobile-devices"
      aria-labelledby="mobile-connected-title"
    >
      <h4 id="mobile-connected-title">
        已授权设备 <span>{{ mobile.status.devices.length }} / 4</span>
      </h4>
      <p v-if="!mobile.status.devices.length" class="settings-row-sub">
        尚未连接设备。
      </p>
      <div
        v-for="device in mobile.status.devices"
        :key="device.id"
        class="mobile-device"
      >
        <div>
          <strong>{{ device.name }}</strong
          ><span
            >{{ device.connected === false ? "待重连" : device.ip
            }}{{ device.remembered ? " · 已记住" : "" }}</span
          >
        </div>
        <button
          class="btn settings-action"
          type="button"
          :disabled="mobile.busy"
          :aria-label="`${device.remembered ? '撤销授权' : '断开'}${device.name}`"
          @click="mobile.revoke(device.id)"
        >
          {{ device.remembered ? "撤销授权" : "断开" }}
        </button>
      </div>
    </section>
    <p class="mobile-note">
      请在信任的局域网中使用。开关状态会保留，重启后自动恢复；关闭设置窗口会继续提供访问。
      关闭访问或退出应用会断开当前连接。开启记住设备后可免配对重连；电脑 IP
      改变、更换浏览器或清除浏览器数据时可能需要重新配对。
    </p>
  </section>
</template>
<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { useMobileAccessStore } from "../../stores/mobile-access";
const mobile = useMobileAccessStore();
const copied = ref(false),
  copyError = ref("");
let poll: ReturnType<typeof setInterval> | undefined,
  copyTimer: ReturnType<typeof setTimeout> | undefined;
function onToggle(event: Event) {
  const input = event.target as HTMLInputElement;
  const enabled = input.checked;
  input.checked = mobile.status.running;
  void mobile.setEnabled(enabled);
}
function onRememberToggle(event: Event) {
  const input = event.target as HTMLInputElement;
  const enabled = input.checked;
  input.checked = !!mobile.status.rememberDevices;
  void mobile.setRemember(enabled);
}
async function copyLink() {
  copyError.value = "";
  try {
    if (!mobile.status.url) return;
    await navigator.clipboard.writeText(mobile.status.url);
    copied.value = true;
    if (copyTimer) clearTimeout(copyTimer);
    copyTimer = setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch {
    copyError.value = "复制未成功，可手动选择上方链接复制。";
  }
}
onMounted(() => {
  void mobile.init();
  poll = setInterval(() => {
    void mobile.refresh();
  }, 1500);
});
onUnmounted(() => {
  if (poll) clearInterval(poll);
  if (copyTimer) clearTimeout(copyTimer);
});
</script>
<style scoped>
.mobile-settings {
  padding: 18px;
  color: var(--ink);
}
.settings-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 18px;
}
.settings-section-head h3 {
  margin: 0;
  font-size: var(--step-5);
  font-weight: var(--w-semibold);
}
.settings-section-sub,
.settings-row-sub {
  color: var(--ink-3);
  font-size: var(--step-3);
  font-weight: normal;
  line-height: 1.7;
}
.settings-action {
  min-height: 32px;
  border: 1px solid var(--border-soft);
  color: var(--ink-2);
  background: transparent;
}
.settings-action:hover {
  border-color: var(--accent);
  color: var(--ink);
}
.mobile-toggle-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 24px;
}
.mobile-remember-row {
  margin-top: 18px;
  padding-top: 18px;
  border-top: 1px solid var(--border-soft);
}
.mobile-toggle-label {
  font-weight: var(--w-semibold);
  cursor: pointer;
}
.mobile-toggle-row p {
  margin-top: 5px;
  line-height: 1.6;
}
.mobile-toggle-row input {
  appearance: none;
  width: 42px;
  height: 24px;
  flex-shrink: 0;
  border: 1px solid var(--border);
  border-radius: 20px;
  background: var(--surface-3);
  position: relative;
  cursor: pointer;
}
.mobile-toggle-row input:before {
  content: "";
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  background: var(--ink-2);
  border-radius: 50%;
  transition: transform 0.15s;
}
.mobile-toggle-row input:checked {
  background: var(--accent);
  border-color: var(--accent);
}
.mobile-toggle-row input:checked:before {
  transform: translateX(18px);
  background: var(--accent-ink);
}
.mobile-toggle-row input:focus-visible {
  outline: 2px solid var(--focus);
  outline-offset: 4px;
}
.mobile-toggle-row input:disabled {
  opacity: 0.5;
  cursor: default;
}
.mobile-entry {
  display: flex;
  gap: 24px;
  padding: 24px 0;
  align-items: center;
  border-top: 1px solid var(--border-soft);
  margin-top: 20px;
}
.mobile-qr {
  width: 190px;
  flex-shrink: 0;
  background: white;
  padding: 8px;
  border-radius: 8px;
}
.mobile-qr :deep(svg) {
  display: block;
  width: 100%;
  height: auto;
}
.mobile-entry-copy {
  min-width: 0;
  flex: 1;
}
.mobile-entry-copy h4 {
  margin: 0 0 8px;
}
.mobile-entry-copy p {
  font-size: var(--step-4);
  line-height: 1.7;
  color: var(--ink-2);
  margin: 0 0 14px;
}
.mobile-link-label {
  display: block;
  font-size: var(--step-3);
  color: var(--ink-3);
  margin-bottom: 6px;
}
.mobile-entry-copy input {
  width: 100%;
  font: var(--step-3) var(--font-mono);
  padding: 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg);
  color: var(--ink-2);
}
.mobile-actions {
  display: flex;
  gap: 8px;
  margin-top: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.mobile-devices {
  border-top: 1px solid var(--border-soft);
  padding: 18px 0;
}
.mobile-devices h4 {
  margin: 0 0 14px;
  font-size: var(--step-4);
  display: flex;
  justify-content: space-between;
}
.mobile-devices h4 span {
  color: var(--ink-3);
  font-weight: normal;
}
.mobile-device {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 12px 0;
}
.mobile-device strong {
  display: block;
  font-size: var(--step-4);
  overflow-wrap: anywhere;
}
.mobile-device span {
  display: block;
  font: var(--step-3) var(--font-mono);
  color: var(--ink-3);
  margin-top: 5px;
}
.mobile-device .mobile-actions {
  margin: 0;
  flex-shrink: 0;
}
.mobile-note {
  font-size: var(--step-3);
  color: var(--ink-3);
  line-height: 1.8;
  margin-top: 16px;
  max-width: 650px;
}
.mobile-error {
  color: var(--warn);
  font-size: var(--step-4);
  line-height: 1.7;
  margin: 12px 0;
}
@media (max-width: 1050px) {
  .mobile-entry {
    gap: 16px;
  }
  .mobile-qr {
    width: 155px;
  }
}
@media (prefers-reduced-motion: reduce) {
  .mobile-toggle-row input:before {
    transition: none;
  }
}
</style>
