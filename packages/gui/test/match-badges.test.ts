import { describe, expect, test } from 'bun:test';
import type { MatchRecord, VideoItem } from '@wonderful-ui/parser';
import { matchRank, matchMomentBadges } from '../src/utils/match-badges.ts';
import { VALORANT_RANKS } from '../src/utils/generated/valorant-ranks.zh-CN.ts';

function match(patch: Partial<MatchRecord> = {}): MatchRecord {
  return { mode: 'competitive', videos: [], ...patch } as MatchRecord;
}

describe('match rank badges', () => {
  test('shows the recorded rank and actual before/after transitions', () => {
    const same = matchRank(match({ tier: { tier_before: 12, tier_after: 12 } }))!;
    expect(same.label).toBe('黄金一');
    expect(same.entries).toHaveLength(1);
    expect(same.entries[0]!.image).toMatch(/^\/valorant\/ranks\/v1-[a-f0-9]{64}\.webp$/);
    const up = matchRank(match({ tier: { tier_before: 11, tier_after: '12' } }))!;
    expect(up.label).toBe('白银三 → 黄金一');
    expect(up.entries[0]!.image).not.toBe(up.entries[1]!.image);
    expect(matchRank(match({ tier: { tier_before: 13, tier_after: 12 } }))!.label).toBe('黄金二 → 黄金一');
  });

  test('distinguishes explicit unranked from missing, corrupt and unused codes', () => {
    expect(matchRank(match({ tier: { tier_before: 0, tier_after: '0' } }))!.label).toBe('未定级');
    expect(matchRank(match({ tier: { tier_before: 0, tier_after: 9 } }))!.label).toBe('未定级 → 白银一');
    for (const value of [undefined, null, '', ' ', true, false, -1, 1, 2, 28, 12.5, Infinity, '0x0', '12x']) {
      const rank = matchRank(match({ tier: { tier_before: value, tier_after: value } }))!;
      expect(rank.label).toBe('未知');
      expect(rank.entries[0]!.image).toBeNull();
    }
    expect(matchRank(match())!.label).toBe('未知');
    expect(matchRank(match({ tier: { tier_before: 12 } }))!.label).toBe('黄金一 → 未知');
  });

  test('accepts CLI extras, preserves primary precedence and ignores unrelated rank-like fields', () => {
    expect(matchRank(match({ extras: { tier: { tier_before: ' 10 ', tier_after: 10 } } }))!.label).toBe('白银二');
    expect(matchRank(match({ tier: null, extras: { tier: { tier_before: 12, tier_after: 12 } } }))!.label).toBe('未知');
    expect(matchRank(match({ extras: { ss_tier_data: { tier_before: 0, tier_after: 0 }, game_level: '12' } }))!.label).toBe('未知');
    for (const mode of ['unrated', 'deathmatch', 'abilitydraftarena', 'customgame']) {
      expect(matchRank(match({ mode, tier: { tier_before: 12, tier_after: 12 } }))).toBeNull();
    }
  });

  test('bundles every active rank and the unranked icon with distinct identities', () => {
    expect(VALORANT_RANKS.map(rank => rank.tier)).toEqual([0, ...Array.from({ length: 25 }, (_, i) => i + 3)]);
    expect(new Set(VALORANT_RANKS.map(rank => rank.image)).size).toBe(26);
    for (const rank of VALORANT_RANKS) {
      expect(rank.cn.trim()).not.toBe('');
      expect(rank.sourceSha256).toMatch(/^[a-f0-9]{64}$/);
      expect(rank.image).toMatch(/^\/valorant\/ranks\/v1-[a-f0-9]{64}\.webp$/);
    }
  });
});

describe('recorded multikill moment badges', () => {
  test('shows both categories while deduplicating repeated clips', () => {
    const videos = ['四杀时刻', '五杀时刻', '四杀时刻', '五杀时刻'].map(video_type => ({ video_type }) as VideoItem);
    const badges = matchMomentBadges(match({ videos }));
    expect(badges.map(badge => badge.label)).toEqual(['四杀', '五杀']);
    expect(badges.map(badge => badge.tone)).toEqual(['four', 'five']);
    expect(badges.map(badge => badge.description)).toEqual(['含四杀时刻视频', '含五杀时刻视频']);
  });

  test('does not infer moments from total kills, titles or similar text', () => {
    const videos = ['击杀集锦', '死亡集锦', '三杀时刻', '疑似五杀时刻'].map(video_type => ({ video_type, video_name: '五杀' }) as VideoItem);
    expect(matchMomentBadges(match({ videos, stats: { kills: 50 } as MatchRecord['stats'] }))).toEqual([]);
    expect(matchMomentBadges(match())).toEqual([]);
  });
});
