<template>
  <div class="mobile-app">
    <header class="app-head">
      <div class="brand">
        <img :src="brandLogo" width="30" height="30" alt="" /><span
          >Wonderful<span class="brand-red">UI</span
          ><small>手机回放</small></span
        >
      </div>
      <button
        v-if="phase === 'library'"
        class="quiet"
        :disabled="busy"
        @click="refreshLibrary"
      >
        {{ busy ? "刷新中" : "刷新" }}
      </button>
    </header>
    <main v-if="phase !== 'library'" class="connect-page">
      <section class="connect-card" aria-labelledby="connect-title">
        <div class="connect-symbol" aria-hidden="true">
          <svg
            viewBox="0 0 48 48"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <rect x="15" y="5" width="20" height="37" rx="4" />
            <path d="M21 11h8m-6 25h4M6 17l-4 5 4 5m34-10 4 5-4 5" />
          </svg>
        </div>
        <p class="eyebrow">WONDERFULUI</p>
        <h1 id="connect-title">
          {{
            phase === "pending"
              ? "等待电脑确认"
              : phase === "checking"
                ? "正在连接电脑"
                : "连接你的电脑"
          }}
        </h1>
        <p class="muted">
          {{
            phase === "pending"
              ? "请在电脑的“设置 → 手机访问”中允许这台设备连接。"
              : "在同一局域网中，浏览和回看电脑上的高光。"
          }}
        </p>
        <p v-if="error" class="notice" role="alert">{{ error }}</p>
        <form v-if="phase === 'pair' && invite" @submit.prevent="beginPair">
          <label for="device-name">这台设备的名称</label>
          <input
            id="device-name"
            v-model="deviceName"
            maxlength="40"
            autocomplete="off"
            required
          />
          <button class="primary wide" :disabled="busy">
            {{ busy ? "正在请求" : "请求连接" }}
          </button>
          <p class="fine">电脑确认前，手机无法读取资料库。</p>
        </form>
        <template v-else-if="phase === 'pair'"
          ><p class="muted">在电脑上开启“手机访问”，然后重新扫描二维码。</p>
          <button class="quiet wide" :disabled="busy" @click="checkConnection">
            重新连接
          </button></template
        >
        <div v-else class="waiting" role="status">
          <span class="spinner" aria-hidden="true" />{{
            phase === "pending" ? "连接请求已发送" : "正在检查连接"
          }}
        </div>
      </section>
    </main>
    <main v-else-if="selected" class="detail-page">
      <button class="back quiet" @click="backToList">‹ 返回对局列表</button>
      <section class="match-summary">
        <img
          v-if="heroImage(selected)"
          :src="heroImage(selected)"
          class="detail-hero"
          alt=""
          @error="hideBrokenImage"
        />
        <div>
          <h1>{{ agentCn(selected) }}</h1>
          <p class="muted">
            {{ mapCn(selected)
            }}<template v-if="modeCn(selected)">
              · {{ modeCn(selected) }}</template
            >
          </p>
          <p class="fine">
            {{ formatTime(selected.matches_time) }} ·
            {{ accountName(selected.openID) }}
          </p>
        </div>
        <span :class="['result', result(selected).className]"
          >{{ result(selected).label }} {{ result(selected).score }}</span
        >
      </section>
      <div class="stat-grid">
        <div v-for="stat in detailStats" :key="stat.label">
          <strong>{{ stat.value }}</strong
          ><span>{{ stat.label }}</span>
        </div>
      </div>
      <p v-if="statsNote(selected)" class="fine">{{ statsNote(selected) }}</p>
      <p v-if="rank(selected)" class="rank-line">
        <template v-for="(entry, i) in rank(selected)!.entries" :key="i"
          ><span v-if="i">→</span
          ><img
            v-if="entry.image"
            :src="entry.image"
            alt=""
            @error="hideBrokenImage"
          />{{ entry.label }}</template
        >
      </p>
      <div class="badges">
        <span
          v-for="badge in moments(selected)"
          :key="badge.type"
          :class="['moment', badge.tone]"
          >{{ badge.label }}</span
        >
      </div>
      <section v-if="playing" class="player-block" aria-label="视频播放">
        <div class="section-head">
          <h2>{{ playing.video_type }}</h2>
          <button class="quiet" @click="stopVideo">关闭视频</button>
        </div>
        <video
          ref="videoElement"
          :key="playing.video_id"
          :src="playing.video_src"
          :poster="playing.video_poster || undefined"
          controls
          playsinline
          preload="metadata"
          @error="videoFailed"
        />
        <p v-if="videoError" class="notice" role="alert">{{ videoError }}</p>
      </section>
      <div class="section-head">
        <h2>本场视频</h2>
        <span class="muted">{{ selected.videos.length }} 段</span>
      </div>
      <div class="video-grid">
        <button
          v-for="video in selected.videos"
          :key="video.video_id"
          class="video-card"
          :disabled="!video.video_src || video.video_isProcessing"
          @click="play(video)"
        >
          <div class="video-cover">
            <img
              v-if="video.video_poster"
              :src="video.video_poster"
              alt=""
              loading="lazy"
              @error="hideBrokenImage"
            /><svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="m9 5 10 7-10 7Z" fill="currentColor" />
            </svg>
          </div>
          <strong>{{ video.video_type || "高光视频" }}</strong
          ><span class="fine">{{
            !video.video_src
              ? "视频未找到"
              : video.video_isProcessing
                ? "正在生成"
                : `${duration(video.video_duration)} · ${video.video_level || "原画"}`
          }}</span>
        </button>
      </div>
      <p v-if="!selected.videos.length" class="empty">本场暂无可用视频。</p>
    </main>
    <main v-else class="library-page">
      <div class="browse-controls">
        <label class="sr-only" for="account-scope">选择账号</label
        ><select id="account-scope" v-model="accountId">
          <option value="">全部账号</option>
          <option
            v-for="a in library.accounts"
            :key="a.openid"
            :value="a.openid"
          >
            {{ label(a) }}
          </option>
        </select>
        <button class="filter-button" @click="openFilters">
          筛选<span v-if="filterCount" class="count-dot">{{
            filterCount
          }}</span>
        </button>
        <label class="search-field"
          ><span class="sr-only">搜索英雄、地图、模式</span
          ><input
            v-model="filters.query"
            type="search"
            placeholder="搜索英雄 / 地图 / 模式"
        /></label>
      </div>
      <p v-if="error" class="notice" role="alert">{{ error }}</p>
      <p v-if="library.totalErrors" class="fine library-note">
        部分源记录未能读取，当前显示可用资料。
      </p>
      <div class="section-head">
        <h1>对局回放</h1>
        <span class="muted" aria-live="polite"
          >{{ filtered.length }} 场 · 时间倒序</span
        >
      </div>
      <div v-if="!filtered.length" class="empty">
        <h2>{{ scoped.length ? "没有符合条件的对局" : "暂无对局" }}</h2>
        <p class="muted">
          {{
            scoped.length
              ? "调整或清除筛选后再试。"
              : "请在电脑上扫描或导入回放资料。"
          }}
        </p>
        <button v-if="filterCount" class="quiet" @click="clearFilters">
          清除筛选
        </button>
      </div>
      <div class="match-list">
        <button
          v-for="match in visible"
          :key="match.matches_id"
          class="match-card"
          @click="openMatch(match)"
        >
          <div class="match-cover">
            <img
              v-if="mapImage(match)"
              :src="mapImage(match)"
              alt=""
              loading="lazy"
              @error="hideBrokenImage"
            /><img
              v-if="heroImage(match)"
              class="hero-portrait"
              :src="heroImage(match)"
              alt=""
              loading="lazy"
              @error="hideBrokenImage"
            /><span
              v-if="achievement(match) !== 'other'"
              :class="['achievement', achievement(match)]"
              >{{ achievement(match).toUpperCase() }}</span
            >
          </div>
          <div class="match-info">
            <div class="match-title">
              <strong>{{ agentCn(match) }}</strong
              ><span :class="['result', result(match).className]"
                >{{ result(match).label }} {{ result(match).score }}</span
              >
            </div>
            <p>
              {{ mapCn(match) }}
              <span v-if="modeCn(match)" class="fine"
                >· {{ modeCn(match) }}</span
              >
            </p>
            <div class="match-kda">
              <span
                >{{ statText(match, "kills") }}/{{
                  statText(match, "deaths")
                }}/{{ statText(match, "assists") }}</span
              ><span
                v-for="badge in moments(match)"
                :key="badge.type"
                :class="['moment', badge.tone]"
                >{{ badge.label }}</span
              >
            </div>
            <p class="fine">
              {{ formatTime(match.matches_time) }} ·
              {{ accountName(match.openID) }}
            </p>
          </div>
        </button>
      </div>
      <button
        v-if="visible.length < filtered.length"
        class="quiet load-more"
        @click="limit += 40"
      >
        加载更多（剩余 {{ filtered.length - visible.length }} 场）
      </button>
    </main>
    <dialog
      ref="filterDialog"
      class="filter-sheet"
      aria-labelledby="filter-title"
      @click="onBackdropClick"
    >
      <div class="sheet-head">
        <h2 id="filter-title">筛选回放</h2>
        <button class="quiet" @click="filterDialog?.close()">取消</button>
      </div>
      <div class="sheet-body">
        <p v-if="filterError" class="notice" role="alert">{{ filterError }}</p>
        <section
          v-for="group in filterGroups"
          :key="group.key"
          class="filter-group"
        >
          <h3>{{ group.label }}</h3>
          <div class="chips">
            <button
              v-for="item in group.items"
              :key="item.value"
              :aria-pressed="
                (draft[group.key] as string[]).includes(item.value)
              "
              @click="toggleCategory(group.key, item.value)"
            >
              {{ item.label }} <span>{{ item.count }}</span>
            </button>
            <p v-if="!group.items.length" class="fine">当前范围暂无选项</p>
          </div>
        </section>
        <section class="filter-group">
          <h3>日期</h3>
          <div class="date-grid">
            <label>开始日期<input v-model="dateFrom" type="date" /></label
            ><label>结束日期<input v-model="dateTo" type="date" /></label>
          </div>
        </section>
        <section class="filter-group">
          <h3>表现筛选</h3>
          <div v-for="range in rangeFields" :key="range.key" class="range-row">
            <span>{{ range.label }}</span
            ><input
              v-model="rangeDraft[range.key][0]"
              type="text"
              :inputmode="range.key === 'kda' ? 'decimal' : 'numeric'"
              :aria-label="`${range.label}最小值`"
              placeholder="不限"
            /><span>至</span
            ><input
              v-model="rangeDraft[range.key][1]"
              type="text"
              :inputmode="range.key === 'kda' ? 'decimal' : 'numeric'"
              :aria-label="`${range.label}最大值`"
              placeholder="不限"
            />
          </div>
        </section>
      </div>
      <div class="sheet-foot">
        <button class="quiet" @click="resetDraft">重置</button
        ><button class="primary" @click="applyDraft">查看结果</button>
      </div>
    </dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type {
  MatchRecord,
  VideoItem,
  SnapshotMatchDetail,
} from "@wonderful-ui/parser";
import { api, AccessError } from "./api";
import {
  sanitizePersistedFilters,
  activeFilterCount,
  type FilterState,
  type CategoryKey,
  agentCn,
  mapCn,
  modeCn,
  kdaRatio,
} from "../../gui/src/utils/filters";
import {
  applyFilters,
  facetValueCounts,
  pruneUnavailableCategories,
} from "../../gui/src/utils/filter-engine";
import {
  attachMatchMetadata,
  matchAchievementCategory as achievement,
} from "../../gui/src/utils/match-metadata";
import { resolveMatchResult as result } from "../../gui/src/utils/match-result";
import {
  matchRank as rank,
  matchMomentBadges as moments,
} from "../../gui/src/utils/match-badges";
import {
  statText,
  hasKdaStats,
  statsNote,
} from "../../gui/src/utils/match-stats";
import {
  resolveMatchHeroImage as heroImage,
  resolveMatchMapImage as mapImage,
} from "../../gui/src/utils/valorant-assets";

interface Account {
  openid: string;
  nick?: string;
  tag?: string;
  customName?: string;
  achievements?: { matchesId: string; achvType: string }[];
  matchDetails?: SnapshotMatchDetail[];
}
interface Library {
  accounts: Account[];
  matches: MatchRecord[];
  totalErrors: number;
}
const phase = ref<"checking" | "pair" | "pending" | "library">("checking");
const brandLogo = new URL("../../gui/src/assets/logo.svg", import.meta.url)
  .href;
const error = ref(""),
  busy = ref(false),
  videoError = ref("");
const deviceName = ref(
  /Android/i.test(navigator.userAgent) ? "安卓手机" : "手机浏览器",
);
const fragment = new URLSearchParams(location.hash.slice(1));
const invite = ref(fragment.get("pair") ?? "");
if (invite.value) history.replaceState(null, "", location.pathname);
const library = ref<Library>({ accounts: [], matches: [], totalErrors: 0 });
const filters = ref<FilterState>(readFilters()),
  draft = ref<FilterState>(sanitizePersistedFilters(null));
const accountId = ref(""),
  selectedId = ref(fragment.get("match") ?? ""),
  limit = ref(40);
const selected = computed(() =>
  library.value.matches.find((m) => m.matches_id === selectedId.value),
);
const playing = ref<VideoItem | null>(null),
  videoElement = ref<HTMLVideoElement | null>(null),
  filterDialog = ref<HTMLDialogElement | null>(null);
const scoped = computed(() =>
  library.value.matches.filter(
    (m) => !accountId.value || m.openID === accountId.value,
  ),
);
const filtered = computed(() =>
  applyFilters(scoped.value, filters.value).sort(
    (a, b) => b.matches_time - a.matches_time,
  ),
);
const visible = computed(() => filtered.value.slice(0, limit.value));
const filterCount = computed(() => activeFilterCount(filters.value));
const dateFrom = ref(""),
  dateTo = ref("");
const filterError = ref("");
const rangeFields = [
  { key: "kills", label: "击杀" },
  { key: "kda", label: "KDA" },
  { key: "videoCount", label: "视频数" },
] as const;
const rangeDraft = ref<
  Record<"kills" | "kda" | "videoCount", [string | number, string | number]>
>({ kills: ["", ""], kda: ["", ""], videoCount: ["", ""] });
const detailStats = computed(() =>
  selected.value
    ? [
        { label: "击杀", value: statText(selected.value, "kills") },
        { label: "死亡", value: statText(selected.value, "deaths") },
        { label: "助攻", value: statText(selected.value, "assists") },
        {
          label: "KDA",
          value: hasKdaStats(selected.value) ? kdaRatio(selected.value) : "—",
        },
      ]
    : [],
);
const labels: Record<string, string> = {
  win: "胜",
  loss: "负",
  draw: "平",
  unknown: "—",
  mvp: "MVP",
  svp: "SVP",
  other: "其他",
  custom: "自定义",
  "non-custom": "非自定义",
};
const groups: { key: CategoryKey; label: string }[] = [
  { key: "sources", label: "来源设备" },
  { key: "heroes", label: "英雄" },
  { key: "maps", label: "地图" },
  { key: "modes", label: "模式" },
  { key: "roomTypes", label: "房间类型" },
  { key: "results", label: "胜负" },
  { key: "achievements", label: "成就" },
  { key: "videoTypes", label: "视频类型" },
];
const filterGroups = computed(() =>
  groups.map((group) => ({
    ...group,
    items: [...facetValueCounts(scoped.value, draft.value, group.key)].map(
      ([value, count]) => ({
        value,
        count,
        label:
          group.key === "sources"
            ? sourceName(value)
            : (labels[value] ?? value),
      }),
    ),
  })),
);
let disposed = false,
  timer: ReturnType<typeof setInterval> | undefined,
  heartbeatBusy = false;
let pending: { requestId: string; requestToken: string } | null = null;
function label(a: Account) {
  return (
    a.customName?.trim() ||
    (a.nick ? `${a.nick}${a.tag ? "#" + a.tag : ""}` : "未命名账号")
  );
}
function accountName(id: string) {
  const a = library.value.accounts.find((a) => a.openid === id);
  return a ? label(a) : "未命名账号";
}
function sourceName(id: string) {
  for (const m of library.value.matches) {
    const s = m.library_sources?.find((s) => s.id === id);
    if (s) return s.label;
  }
  return "来源设备";
}
function formatTime(time: number) {
  return new Date(time).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}
function duration(ms: number) {
  const sec = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, "0")}`;
}
function hideBrokenImage(event: Event) {
  (event.target as HTMLImageElement).style.visibility = "hidden";
}
function readFilters() {
  try {
    return sanitizePersistedFilters(
      JSON.parse(localStorage.getItem("wui:mobile.filters.v1") ?? "null"),
    );
  } catch {
    return sanitizePersistedFilters(null);
  }
}
watch(
  filters,
  (value) => {
    limit.value = 40;
    try {
      localStorage.setItem("wui:mobile.filters.v1", JSON.stringify(value));
    } catch {}
  },
  { deep: true },
);
watch(accountId, () => {
  limit.value = 40;
});
function stopVideo() {
  videoElement.value?.pause();
  playing.value = null;
  videoError.value = "";
}
function play(video: VideoItem) {
  videoError.value = "";
  playing.value = video;
  void nextTick(() => {
    void videoElement.value?.play().catch(() => {});
  });
}
function videoFailed() {
  videoError.value =
    "此视频暂时无法播放，可能是手机不支持原视频编码，或文件已移动。请尝试安卓 Chrome，或在电脑上检查视频。";
}
function openMatch(match: MatchRecord) {
  stopVideo();
  selectedId.value = match.matches_id;
  history.pushState(
    { match: match.matches_id },
    "",
    `#match=${match.matches_id}`,
  );
  window.scrollTo(0, 0);
}
function backToList() {
  stopVideo();
  if (history.state?.match) history.back();
  else {
    selectedId.value = "";
    history.replaceState(null, "", location.pathname);
  }
}
function routeChanged() {
  stopVideo();
  selectedId.value =
    new URLSearchParams(location.hash.slice(1)).get("match") ?? "";
  filterDialog.value?.close();
}
function disconnected(message: string) {
  stopVideo();
  filterDialog.value?.close();
  library.value = { accounts: [], matches: [], totalErrors: 0 };
  phase.value = "pair";
  error.value = message;
  pending = null;
  try {
    sessionStorage.removeItem("wui:mobile.pending");
  } catch {}
}
async function checkConnection() {
  busy.value = true;
  try {
    await refreshLibrary();
  } finally {
    busy.value = false;
  }
}
async function refreshLibrary() {
  if (busy.value && phase.value === "library") return;
  busy.value = true;
  try {
    const data = await api<Library>("/api/library");
    if (disposed) return;
    attachMatchMetadata(data.matches, data.accounts);
    library.value = data;
    filters.value = pruneUnavailableCategories(data.matches, filters.value);
    if (
      accountId.value &&
      !data.accounts.some((a) => a.openid === accountId.value)
    )
      accountId.value = "";
    phase.value = "library";
    error.value = "";
    invite.value = "";
    pending = null;
    try {
      sessionStorage.removeItem("wui:mobile.pending");
    } catch {}
  } catch (e) {
    if (disposed) return;
    const failure = e as AccessError;
    if (
      (failure.status === 401 || failure.status === 403) &&
      phase.value === "library"
    ) {
      disconnected(failure.message);
    } else if (phase.value !== "library") {
      phase.value = "pair";
      if (!invite.value) error.value = failure.message;
    } else {
      error.value = failure.message;
    }
  } finally {
    busy.value = false;
  }
}
async function beginPair() {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    pending = await api("/api/pair", {
      invite: invite.value,
      name: deviceName.value,
    });
    if (disposed) return;
    phase.value = "pending";
    invite.value = "";
    try {
      sessionStorage.setItem("wui:mobile.pending", JSON.stringify(pending));
    } catch {}
  } catch (e) {
    error.value = (e as Error).message;
  } finally {
    busy.value = false;
  }
}
async function heartbeat() {
  if (heartbeatBusy || disposed) return;
  heartbeatBusy = true;
  try {
    if (phase.value === "pending" && pending) {
      const status = await api<{ status: string }>("/api/pair/status", pending);
      if (status.status === "approved") await refreshLibrary();
    } else if (phase.value === "library") {
      await api("/api/session");
    }
  } catch (e) {
    if (!disposed) disconnected((e as Error).message);
  } finally {
    heartbeatBusy = false;
  }
}
function openFilters() {
  filterError.value = "";
  draft.value = sanitizePersistedFilters(filters.value);
  dateFrom.value = dateString(draft.value.dateRange[0]);
  dateTo.value = dateString(draft.value.dateRange[1]);
  for (const field of rangeFields) {
    rangeDraft.value[field.key] = draft.value[field.key].map(
      (n) => n ?? "",
    ) as [number | string, number | string];
  }
  filterDialog.value?.showModal();
}
function dateString(ms: number | null) {
  if (ms === null) return "";
  const date = new Date(ms);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
function toggleCategory(key: CategoryKey, value: string) {
  const current = draft.value[key] as string[];
  (draft.value as unknown as Record<string, string[]>)[key] = current.includes(
    value,
  )
    ? current.filter((v) => v !== value)
    : [...current, value];
}
function resetDraft() {
  filterError.value = "";
  draft.value = sanitizePersistedFilters(null);
  dateFrom.value = "";
  dateTo.value = "";
  for (const field of rangeFields) rangeDraft.value[field.key] = ["", ""];
}
function clearFilters() {
  filters.value = sanitizePersistedFilters(null);
}
function applyDraft() {
  const next = sanitizePersistedFilters(draft.value);
  next.query = filters.value.query;
  for (const field of rangeFields) {
    const values = rangeDraft.value[field.key].map((v) =>
      String(v).trim() === "" ? null : Number(v),
    );
    if (
      values.some(
        (v) =>
          v !== null &&
          (!Number.isFinite(v) ||
            v < 0 ||
            (field.key !== "kda" && !Number.isInteger(v))),
      )
    ) {
      filterError.value =
        "表现筛选请输入有效的非负数，击杀和视频数需要填写整数。";
      return;
    }
    next[field.key] = values as [number | null, number | null];
  }
  next.dateRange = [
    dateFrom.value ? new Date(dateFrom.value + "T00:00:00").getTime() : null,
    dateTo.value ? new Date(dateTo.value + "T23:59:59.999").getTime() : null,
  ];
  filters.value = sanitizePersistedFilters(next);
  filterDialog.value?.close();
}
function onBackdropClick(e: MouseEvent) {
  const dialog = filterDialog.value;
  if (dialog && e.target === dialog) {
    const rect = dialog.getBoundingClientRect();
    if (
      e.clientX < rect.left ||
      e.clientX > rect.right ||
      e.clientY < rect.top ||
      e.clientY > rect.bottom
    )
      dialog.close();
  }
}
onMounted(async () => {
  window.addEventListener("popstate", routeChanged);
  try {
    const saved = JSON.parse(
      sessionStorage.getItem("wui:mobile.pending") ?? "null",
    );
    if (saved?.requestId && saved?.requestToken) pending = saved;
  } catch {}
  await refreshLibrary();
  if (disposed) return;
  if (phase.value === "pair" && pending) {
    phase.value = "pending";
    error.value = "";
  }
  timer = setInterval(heartbeat, 1500);
});
onUnmounted(() => {
  disposed = true;
  stopVideo();
  if (timer) clearInterval(timer);
  window.removeEventListener("popstate", routeChanged);
});
</script>
