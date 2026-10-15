import { describe, test, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createTestingPinia } from '@pinia/testing';
import FilterBar from '../src/components/match/FilterBar.vue';
import type { MatchRecord } from '@wonderful-ui/parser';
import { useFilterStore } from '../src/stores/filter';

function mkMatch(openid = 'test-1', hero = 'Cypher', mapId = 'Ascent', kills = 10): MatchRecord {
  return {
    openID: openid,
    matches_id: `match-${Math.random().toString(36).slice(2, 8)}`,
    matches_time: 1719000000000,
    map: { map_id: `/Game/Maps/${mapId}/${mapId}` },
    mode: 'competitive',
    agent: { agent_name: hero, agent_id: `${hero}-id` },
    career: {},
    stats: { kills, deaths: 5, assists: 3, score: 2000, has_won: true, rounds_won: 13, rounds_lost: 5, mode_name: '', game_level: '' },
    minRoundId: 0,
    gameStartTime: '2026-06-08 18:00:00',
    gameEndTime: '2026-06-08 18:30:00',
    videos: [],
  } as MatchRecord;
}

function mountFilter(overrides: { matches?: MatchRecord[]; filters?: Record<string, unknown> } = {}) {
  const matches = overrides.matches ?? [mkMatch()];
  return mount(FilterBar, {
    global: {
      plugins: [createTestingPinia({
        createSpy: vi.fn,
        stubActions: false,
        initialState: {
          account: {
            accounts: [{ openid: 'test-1', path: '', matchCount: matches.length }],
            selectedAccountId: '__all__',
            matches,
            accountLabels: new Map(),
            assetPathCache: new Map(),
          },
          filter: {
            filters: {
              heroes: [], maps: [], modes: [], roomTypes: [], results: [], achievements: [], videoTypes: [],
              dateRange: [null, null], query: '',
              kills: [null, null], deaths: [null, null], assists: [null, null],
              kda: [null, null], score: [null, null], roundsWon: [null, null], videoCount: [null, null],
              ...overrides.filters,
            },
            filterBarOpen: false,
          },
        },
      })],
    },
  });
}

describe('FilterBar', () => {
  test('room type sits between mode and result with single selection and an all reset', async () => {
    const custom = { ...mkMatch(), mode: 'customgame', career: { game_mode: '团队乱斗' } };
    const regular = { ...mkMatch(), mode: 'hurm', career: { game_mode: '团队乱斗' } };
    const other = { ...mkMatch(), mode: 'competitive' };
    const wrapper = mountFilter({ matches: [custom, regular, other], filters: { modes: ['团队乱斗'] } });
    const keys = wrapper.findAll('[data-filter-key]').map(el => el.attributes('data-filter-key'));
    expect(keys.indexOf('roomTypes')).toBe(keys.indexOf('modes') + 1);
    expect(keys.indexOf('results')).toBe(keys.indexOf('roomTypes') + 1);
    const section = wrapper.get('[data-filter-key="roomTypes"]');
    expect(section.text()).toContain('房间类型');
    expect(section.get('[data-value="all"]').text()).toBe('全部 2');
    expect(section.get('[data-value="all"]').attributes('aria-pressed')).toBe('true');
    expect(section.get('[data-value="custom"]').text()).toBe('自定义 1');
    expect(section.get('[data-value="non-custom"]').text()).toBe('非自定义 1');
    await section.get('[data-value="custom"]').trigger('click');
    expect(useFilterStore().filters.roomTypes).toEqual(['custom']);
    expect(section.get('[data-value="custom"]').attributes('aria-pressed')).toBe('true');
    await section.get('[data-value="non-custom"]').trigger('click');
    expect(useFilterStore().filters.roomTypes).toEqual(['non-custom']);
    expect(section.get('[data-value="custom"]').attributes('aria-pressed')).toBe('false');
    await section.get('[data-value="all"]').trigger('click');
    expect(useFilterStore().filters.roomTypes).toEqual([]);
    expect(useFilterStore().filters.modes).toEqual(['团队乱斗']);
    wrapper.unmount();
  });
  test('achievement chips group ordinary and absent metadata under other', async () => {
    const matches = [
      { ...mkMatch(), library_metadata: { achievement: 'mvp' as const } },
      { ...mkMatch(), library_metadata: { achievement: 'svp' as const } },
      { ...mkMatch(), library_metadata: { achievement: 'normal' as const } },
      mkMatch(),
    ];
    const wrapper = mountFilter({ matches });
    const chips = wrapper.get('[data-filter-key="achievements"]').findAll('button');
    expect(chips.map(chip => chip.text())).toEqual(['MVP 1', 'SVP 1', '其他 2']);
    await chips[2]!.trigger('click');
    expect(useFilterStore().filters.achievements).toEqual(['other']);
    expect(chips[2]!.attributes('aria-pressed')).toBe('true');
    expect(useFilterStore().applyToMatches(matches)).toEqual(matches.slice(2));
    useFilterStore().clearFilter('__all__');
    await wrapper.vm.$nextTick();
    expect(chips[2]!.attributes('aria-pressed')).toBe('false');
    wrapper.unmount();
  });
  test('shows source labels and counts a shared match in both devices', async () => {
    const m = mkMatch();
    m.library_sources = [{ id: 'local', label: '本机' }, { id: 'import:fixture', label: '旧电脑' }];
    const wrapper = mountFilter({ matches: [m] });
    expect(wrapper.get('[data-value="local"]').text()).toBe('本机 1');
    expect(wrapper.get('[data-value="import:fixture"]').text()).toBe('导入设备 · 旧电脑 1');
    await wrapper.get('[data-value="import:fixture"]').trigger('click');
    expect(useFilterStore().filters.sources).toEqual(['import:fixture']);
    wrapper.unmount();
  });
  test('shows multikill chips and allows multiple selections and clearing', async () => {
    const m = mkMatch();
    m.videos = ['击杀集锦', '三杀时刻', '四杀时刻', '五杀时刻']
      .map((video_type, i) => ({ video_id: `video-${i}`, video_type })) as MatchRecord['videos'];
    const wrapper = mountFilter({ matches: [m] });
    for (const type of ['三杀时刻', '四杀时刻', '五杀时刻']) {
      const chip = wrapper.get(`[data-filter-key="videoTypes"] [data-value="${type}"]`);
      expect(chip.text()).toBe(`${type} 1`);
    }
    const triple = wrapper.get('[data-value="三杀时刻"]');
    const quad = wrapper.get('[data-value="四杀时刻"]');
    await triple.trigger('click');
    await quad.trigger('click');
    expect(triple.attributes('aria-pressed')).toBe('true');
    expect(quad.attributes('aria-pressed')).toBe('true');
    expect(useFilterStore().filters.videoTypes).toEqual(['三杀时刻', '四杀时刻']);
    useFilterStore().clearFilter('__all__');
    await wrapper.vm.$nextTick();
    expect(triple.attributes('aria-pressed')).toBe('false');
    expect(quad.attributes('aria-pressed')).toBe('false');
    wrapper.unmount();
  });

  test('renders without crashing', () => {
    const wrapper = mountFilter();
    expect(wrapper.find('.filter-section').exists()).toBe(true);
  });

  test('renders hero and map sections', () => {
    const m1 = mkMatch('a', 'Cypher', 'Ascent');
    const m2 = { ...mkMatch('a', 'Omen', 'Bind'), agent: { agent_name: 'Omen', agent_id: 'o-id' }, career: { hero_name: '幽影', map_name: '源工重镇' } };
    const wrapper = mountFilter({ matches: [m1, m2 as MatchRecord] });
    expect(wrapper.text()).toContain('英雄');
    expect(wrapper.text()).toContain('地图');
  });

  test('active filter chip has is-active class', () => {
    const m = mkMatch('a', 'Cypher', 'Ascent');
    const wrapper = mountFilter({
      filters: { heroes: ['零'] },
      matches: [m],
    });
    const chip = wrapper.find('.filter-chip.is-active');
    expect(chip.exists()).toBe(true);
    expect(chip.attributes('aria-pressed')).toBe('true');
  });

  test('renders date section', () => {
    const wrapper = mountFilter();
    expect(wrapper.text()).toContain('日期');
  });

  test('renders performance filter when multiple matches provide range', () => {
    const matches = [mkMatch('a', 'Cypher', 'Ascent', 5), mkMatch('a', 'Omen', 'Bind', 20)];
    const wrapper = mountFilter({ matches });
    expect(wrapper.text()).toContain('表现筛选');
  });

  test('toggles performance filter expand', async () => {
    const matches = [mkMatch('a', 'Cypher', 'Ascent', 5), mkMatch('a', 'Omen', 'Bind', 20)];
    const wrapper = mountFilter({ matches });
    const body = wrapper.find('.filter-num-group-body');
    expect(body.classes()).toContain('is-collapsed');
    expect(body.attributes('aria-hidden')).toBe('true');
    expect(body.attributes()).toHaveProperty('inert');
    await wrapper.find('.filter-num-group-header').trigger('click');
    expect(body.classes()).not.toContain('is-collapsed');
    expect(body.attributes('aria-hidden')).toBe('false');
    expect(body.attributes()).not.toHaveProperty('inert');
  });
});
