import { describe, test, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createTestingPinia } from '@pinia/testing';
import MatchCard from '../src/components/match/MatchCard.vue';
import type { MatchRecord } from '@wonderful-ui/parser';

function mkMatch(overrides: Partial<MatchRecord> = {}): MatchRecord {
  return {
    openID: 'test-openid',
    matches_id: 'match-001',
    matches_time: 1719000000000,
    map: { map_id: '/Game/Maps/Ascent/Ascent' },
    mode: 'competitive',
    agent: { agent_name: 'Cypher', agent_id: 'cypher-id' },
    career: { hero_name: '零', map_name: '亚海悬城' },
    stats: { kills: 14, deaths: 12, assists: 5, score: 3200, has_won: true, rounds_won: 13, rounds_lost: 10, mode_name: '', game_level: '' },
    minRoundId: 0,
    gameStartTime: '2026-06-08 18:00:00',
    gameEndTime: '2026-06-08 18:35:00',
    videos: [],
    ...overrides,
  } as MatchRecord;
}

function mountCard(
  match: MatchRecord,
  accountLabel = 'test',
  isSelected = false,
  isFocused = false,
) {
  return mount(MatchCard, {
    props: { match, isSelected, isFocused, accountLabel },
    global: {
      plugins: [createTestingPinia({
        createSpy: vi.fn,
        stubActions: false,
        initialState: {
          account: {
            accounts: [],
            assetPathCache: new Map(),
            matchAchievements: new Map(),
          },
        },
      })],
    },
  });
}

describe('MatchCard', () => {
  test('shows both recorded moments and rank with separate labels and local icons', () => {
    const videos = ['四杀时刻', '五杀时刻', '五杀时刻'].map(video_type => ({ video_type })) as MatchRecord['videos'];
    const wrapper = mountCard(mkMatch({ tier: { tier_before: 11, tier_after: 12 }, videos }));
    expect(wrapper.get('.match-rank').attributes('aria-label')).toBe('赛前段位：白银三；赛后段位：黄金一');
    expect(wrapper.findAll('.rank-icon')).toHaveLength(2);
    for (const icon of wrapper.findAll('.rank-icon')) expect(icon.attributes('src')).toMatch(/^\/valorant\/ranks\//);
    expect(wrapper.get('.moment-four').text()).toBe('四杀');
    expect(wrapper.get('.moment-five').text()).toBe('五杀');
    expect(wrapper.findAll('.match-moment')).toHaveLength(2);
    expect(wrapper.get('.moment-four').attributes('data-tip')).toBe('含四杀时刻视频');
    expect(wrapper.attributes('aria-label')).toContain('含五杀时刻视频');
    expect(wrapper.get('.match-kda').text()).toBe('14/12/5');
    wrapper.unmount();
  });

  test('unranked and unknown remain distinct, and missing icons keep known rank text', async () => {
    const wrapper = mountCard(mkMatch({ tier: { tier_before: 0, tier_after: 0 } }));
    expect(wrapper.get('.match-rank').text()).toBe('未定级');
    await wrapper.get('.rank-icon').trigger('error');
    expect(wrapper.get('.match-rank').text()).toContain('未定级');
    expect(wrapper.find('.rank-icon').exists()).toBe(false);
    await wrapper.setProps({ match: mkMatch({ tier: null }) });
    expect(wrapper.get('.match-rank').text()).toContain('未知');
    expect(wrapper.find('.rank-icon').exists()).toBe(false);
    await wrapper.setProps({ match: mkMatch({ tier: { tier_before: 12, tier_after: 12 } }) });
    expect(wrapper.find('.rank-icon').exists()).toBe(true);
    await wrapper.setProps({ match: mkMatch({ mode: 'abilitydraftarena' }) });
    expect(wrapper.find('.match-rank').exists()).toBe(false);
    wrapper.unmount();
  });
  test('robot uses a bundled portrait and uncertain all-zero stats retain source values with a hint', () => {
    const m = mkMatch({ mode: 'abilitydraftarena', agent: { agent_name: 'AbilityDraftAgent', agent_id: '' }, career: {} });
    m.stats = { ...m.stats, has_won: false, result_known: true, kills: 0, deaths: 0, assists: 0, score: 0 };
    const wrapper = mountCard(m);
    expect(wrapper.get('.hero-img').attributes('src')).toMatch(/^\/valorant\/agents\/v1-[a-f0-9]{64}\.webp$/);
    expect(wrapper.get('.hero-img').attributes('alt')).toBe('轮战机器人');
    expect(wrapper.find('.hero-placeholder').exists()).toBe(false);
    expect(wrapper.get('.match-kda').text()).toBe('0/0/0');
    expect(wrapper.get('.match-kda').attributes('data-tip')).toContain('统计可能不完整');
    expect(wrapper.get('.result-unknown').text()).toBe('—');
  });
  test('draw and missing results render neutral pills with accessible explanations', () => {
    const draw = mkMatch(); draw.stats.rounds_won = 12; draw.stats.rounds_lost = 12;
    const drawCard = mountCard(draw);
    expect(drawCard.get('.result-draw').text()).toBe('平 12:12');
    expect(drawCard.get('.result-draw').attributes('aria-label')).toBe('平局 12:12');
    const unknown = mkMatch(); unknown.stats = { ...unknown.stats, has_won: false, result_known: false, rounds_won: 0, rounds_lost: 0 };
    const unknownCard = mountCard(unknown);
    expect(unknownCard.get('.result-unknown').text()).toBe('—');
    expect(unknownCard.get('.result-unknown').attributes('data-tip')).toBe('暂无可靠对局结果');
    expect(unknownCard.text()).not.toContain('0:0');
  });

  test('renders canonical agent name', () => {
    const wrapper = mountCard(mkMatch());
    expect(wrapper.text()).toContain('零');
  });

  test('renders KDA text', () => {
    const wrapper = mountCard(mkMatch());
    expect(wrapper.text()).toContain('14/12/5');
  });

  test('renders win result and class', () => {
    const m = mkMatch({ stats: { kills: 10, deaths: 8, assists: 3, score: 2500, has_won: true, rounds_won: 13, rounds_lost: 5, mode_name: '', game_level: '' } });
    const wrapper = mountCard(m);
    expect(wrapper.text()).toContain('胜');
    expect(wrapper.find('.result-win').exists()).toBe(true);
  });

  test('renders loss result and class', () => {
    const m = mkMatch({ stats: { kills: 10, deaths: 8, assists: 3, score: 2500, has_won: false, rounds_won: 5, rounds_lost: 13, mode_name: '', game_level: '' } });
    const wrapper = mountCard(m);
    expect(wrapper.text()).toContain('负');
    expect(wrapper.find('.result-loss').exists()).toBe(true);
  });

  test('applies is-selected class when selected', () => {
    const wrapper = mountCard(mkMatch(), 'test', true);
    expect(wrapper.find('.is-selected').exists()).toBe(true);
  });

  test('no is-selected class when not selected', () => {
    const wrapper = mountCard(mkMatch(), 'test', false);
    expect(wrapper.find('.is-selected').exists()).toBe(false);
  });

  test('renders video count chip', () => {
    const m = mkMatch({ videos: [{ video_id: 'v1' } as any, { video_id: 'v2' } as any] });
    const wrapper = mountCard(m);
    expect(wrapper.text()).toContain('× 2');
  });

  test('emits click on match-row click', async () => {
    const wrapper = mountCard(mkMatch());
    await wrapper.find('.match-row').trigger('click');
    expect(wrapper.emitted('click')).toHaveLength(1);
  });

  test('emits dblclick on double click', async () => {
    const wrapper = mountCard(mkMatch());
    await wrapper.find('.match-row').trigger('dblclick');
    expect(wrapper.emitted('dblclick')).toHaveLength(1);
  });

  test('renders account label', () => {
    const wrapper = mountCard(mkMatch(), '我的大号');
    expect(wrapper.text()).toContain('我的大号');
  });

  test('renders MVP badge when achievement exists', () => {
    const m = mkMatch({ matches_id: 'mvp-match', library_metadata: { achievement: 'mvp' } });
    const wrapper = mount(MatchCard, {
      props: { match: m, isSelected: false, accountLabel: 'test' },
      global: {
        plugins: [createTestingPinia({
          createSpy: vi.fn,
          stubActions: false,
          initialState: {
            account: {
              accounts: [{ openid: 'test', path: '', matchCount: 1, achievements: [{ matchesId: 'mvp-match', achvType: 'mvp', typeStr: 'MVP' }] }],
              assetPathCache: new Map(),
            },
          },
        })],
      },
    });
    expect(wrapper.find('.cover-badge-mvp').exists()).toBe(true);
  });

  test('shows map fallback when no image available', () => {
    // Unknown map_id + empty career → no CDN/career URL → cover fallback glyph.
    const wrapper = mountCard(mkMatch({
      map: { map_id: '/Game/Maps/DoesNotExist/Nope' },
      career: { hero_name: '黑梦', map_name: '', map_image: '', hero_image: '' },
    }));
    expect(wrapper.find('.cover-bg-fallback').exists()).toBe(true);
  });

  test('has data-match-id attribute', () => {
    const wrapper = mountCard(mkMatch({ matches_id: 'abc-123' }));
    expect(wrapper.find('.match-row').attributes('data-match-id')).toBe('abc-123');
  });

  test('exposes stable option id for aria-activedescendant', () => {
    const wrapper = mountCard(mkMatch({ matches_id: 'abc-123' }));
    expect(wrapper.find('.match-row').attributes('id')).toBe('wui-match-opt-abc-123');
    expect(wrapper.find('.match-row').attributes('tabindex')).toBe('-1');
  });

  test('is-focused paints keyboard active class', () => {
    const wrapper = mountCard(mkMatch(), 'test', false, true);
    expect(wrapper.find('.match-row').classes()).toContain('is-focused');
  });

  test('resets image failures when a refresh replaces the same keyed match', async () => {
    const first = mkMatch({
      agent: { agent_name: 'UnknownOne', agent_id: 'unknown-one' },
      map: { map_id: '/Game/Maps/UnknownOne/UnknownOne' },
      career: {
        hero_name: '未知一',
        hero_image: 'data:image/png;base64,first-hero',
        map_name: '未知地图一',
        map_image: 'data:image/png;base64,first-map',
        game_mode: '未知模式一',
        game_mode_icon: 'data:image/png;base64,first-mode',
      },
    });
    const wrapper = mountCard(first);
    await wrapper.get('.cover-bg').trigger('error');
    await wrapper.get('.hero-img').trigger('error');
    await wrapper.get('.mode-icon').trigger('error');
    expect(wrapper.find('.cover-bg').exists()).toBe(false);
    expect(wrapper.find('.hero-img').exists()).toBe(false);
    expect(wrapper.find('.mode-icon').exists()).toBe(false);

    await wrapper.setProps({
      match: mkMatch({
        matches_id: first.matches_id,
        agent: { agent_name: 'UnknownTwo', agent_id: 'unknown-two' },
        map: { map_id: '/Game/Maps/UnknownTwo/UnknownTwo' },
        career: {
          hero_name: '未知二',
          hero_image: 'data:image/png;base64,second-hero',
          map_name: '未知地图二',
          map_image: 'data:image/png;base64,second-map',
          game_mode: '未知模式二',
          game_mode_icon: 'data:image/png;base64,second-mode',
        },
      }),
    });

    expect(wrapper.get('.cover-bg').attributes('src')).toContain('second-map');
    expect(wrapper.get('.hero-img').attributes('src')).toContain('second-hero');
    expect(wrapper.get('.mode-icon').attributes('src')).toContain('second-mode');
    wrapper.unmount();
  });
});
