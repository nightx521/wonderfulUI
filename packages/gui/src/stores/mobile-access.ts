import { defineStore } from "pinia";
import { onScopeDispose, ref } from "vue";
import { invoke, listen, type UnlistenFn } from "../tauri-adapter";

export interface MobileDevice {
  id: string;
  name: string;
  ip: string;
  connected?: boolean;
  remembered?: boolean;
  remainingSeconds?: number;
}
export interface MobileStatus {
  running: boolean;
  generation: string | null;
  url: string | null;
  qrSvg: string | null;
  pending: MobileDevice[];
  devices: MobileDevice[];
  rememberDevices?: boolean;
  restoreError?: string | null;
}
const empty = (): MobileStatus => ({
  running: false,
  generation: null,
  url: null,
  qrSvg: null,
  pending: [],
  devices: [],
});
const safeErrors = new Set([
  "手机访问会话已结束",
  "手机访问已关闭",
  "连接请求已过期",
  "连接请求已处理",
  "连接数量已达上限",
  "未检测到可用的局域网，请连接 Wi-Fi 或有线网络",
  "局域网地址不可用",
  "手机访问端口 22358 无法使用，可能被其它程序占用",
  "手机访问防火墙授权未完成，请允许系统授权后重试",
  "资料库暂时无法读取，请稍后重试",
  "资料库较大，暂时无法在手机中加载",
  "已记住设备的设置无法读取，请检查本地设置",
  "设备授权设置保存失败，请稍后重试",
]);
export const useMobileAccessStore = defineStore("mobile-access", () => {
  const status = ref<MobileStatus>(empty()),
    busy = ref(false),
    error = ref("");
  let epoch = 0,
    unlisten: UnlistenFn | null = null,
    listening: Promise<void> | null = null,
    disposed = false;
  function observe() {
    if (unlisten || listening) return listening ?? Promise.resolve();
    const request = listen<{ pairRequest?: boolean }>(
      "wui://mobile_access_changed",
      (event) => {
        void refresh();
      },
    )
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {})
      .finally(() => {
        listening = null;
      });
    listening = request;
    return request;
  }
  async function refresh() {
    if (busy.value || disposed) return;
    const revision = epoch;
    try {
      const result = await invoke<MobileStatus>("mobile_access_status");
      if (revision === epoch && !disposed) status.value = result;
    } catch {
      /* Keep the last known service state on transient status failures. */
    }
  }
  async function action(command: string, args?: Record<string, unknown>) {
    if (busy.value) return;
    const revision = ++epoch;
    busy.value = true;
    error.value = "";
    try {
      await observe();
      const result = await invoke<MobileStatus>(command, args);
      if (revision === epoch && !disposed) status.value = result;
    } catch (e) {
      if (revision === epoch && !disposed) {
        const text = e instanceof Error ? e.message : String(e);
        error.value = safeErrors.has(text)
          ? text
          : "手机访问操作未完成，请重试。";
      }
    } finally {
      if (revision === epoch) busy.value = false;
    }
  }
  async function setEnabled(enabled: boolean) {
    await action(enabled ? "start_mobile_access" : "stop_mobile_access");
  }
  async function decide(id: string, allow: boolean) {
    await action("decide_mobile_device", {
      generation: status.value.generation,
      id,
      allow,
    });
  }
  async function revoke(id: string) {
    await action("revoke_mobile_device", {
      generation: status.value.generation,
      id,
    });
  }
  async function rotate() {
    await action("refresh_mobile_invite");
  }
  async function setRemember(remember: boolean) {
    await action("set_mobile_remember", { remember });
  }
  async function init() {
    await observe();
    await refresh();
  }
  onScopeDispose(() => {
    disposed = true;
    epoch++;
    unlisten?.();
    unlisten = null;
  });
  return {
    status,
    busy,
    error,
    init,
    refresh,
    setEnabled,
    decide,
    revoke,
    rotate,
    setRemember,
  };
});
