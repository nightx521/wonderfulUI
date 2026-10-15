<template>
  <span class="match-rank" :title="rank.description" :data-tip="rank.description" :aria-label="rank.description">
    <template v-for="(entry, index) in rank.entries" :key="index">
      <span v-if="index" class="rank-arrow" aria-hidden="true">→</span>
      <span class="rank-entry">
        <img v-if="entry.image && !failed[entry.image]" class="rank-icon" :src="entry.image"
          alt="" aria-hidden="true" width="16" height="16" loading="lazy" decoding="async"
          @error="failed[entry.image] = true" />
        <span v-else class="rank-placeholder" aria-hidden="true">?</span>
        <span class="rank-name">{{ entry.label }}</span>
      </span>
    </template>
  </span>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import type { matchRank } from '../../utils/match-badges.ts';

const props = defineProps<{ rank: NonNullable<ReturnType<typeof matchRank>> }>();
const failed = ref<Record<string, boolean>>({});
watch(() => props.rank, () => { failed.value = {}; });
</script>

<style scoped>
.match-rank { display: inline-flex; align-items: center; gap: 4px; min-width: 0; color: var(--ink-2); font-size: 11px; }
.rank-entry { display: inline-flex; align-items: center; gap: 3px; min-width: 0; }
.rank-icon { flex-shrink: 0; width: 16px; height: 16px; object-fit: contain; }
.rank-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rank-arrow { flex-shrink: 0; color: var(--ink-3); font-family: var(--font-mono); }
.rank-placeholder { display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0;
  width: 14px; height: 14px; border: 1px solid var(--ink-4); border-radius: 50%; color: var(--ink-3); font-size: 10px; }
</style>
