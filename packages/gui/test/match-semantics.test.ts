import { describe, expect, test } from 'bun:test';
import type { MatchRecord } from '@wonderful-ui/parser';
import { resolveMatchResult } from '../src/utils/match-result.ts';
import { attachMatchMetadata, matchAchievement } from '../src/utils/match-metadata.ts';
import { applyFilters, facetValueCounts } from '../src/utils/filter-engine.ts';
import { EMPTY_FILTERS, activeFilterSummary, sanitizePersistedFilters } from '../src/utils/filters.ts';
import { resolveMatchAgentLabel, resolveMatchMapLabel } from '../src/utils/valorant-assets.ts';
import { statText, statsNote, hasKdaStats } from '../src/utils/match-stats.ts';

function match(overrides: Partial<MatchRecord> = {}): MatchRecord {
  return { openID: 'fixture-a', matches_id: 'fixture-match', matches_time: 1,
    map: { map_id: '/Game/Maps/Ascent/Ascent' }, agent: { agent_name: 'Jett', agent_id: '' },
    mode: 'competitive', minRoundId: 0, gameStartTime: '', gameEndTime: '', videos: [],
    stats: { kills: 0, deaths: 0, assists: 0, score: 0, mode_name: '', game_level: '',
      has_won: false, rounds_won: 0, rounds_lost: 0 }, ...overrides };
}
function score(won: number, lost: number, stats: Partial<MatchRecord['stats']> = {}, mode = 'competitive') {
  return match({ mode, stats: { ...match().stats, rounds_won: won, rounds_lost: lost, ...stats } });
}

describe('shared match results', () => {
  test('placement results ignore false flags and round scores without match settlement facts', () => {
    for (const mode of ['deathmatch', 'abilitydraftarena']) {
      for (const result_known of [undefined, true, false]) {
        const m = score(0, 17, { result_known }, mode);
        expect(resolveMatchResult(m)).toMatchObject({ outcome: 'unknown', label: '—', score: '' });
        m.library_metadata = { hasWon: false };
        expect(resolveMatchResult(m).outcome).toBe('loss');
      }
      expect(resolveMatchResult(score(0, 0, { has_won: true, result_known: true }, mode)).outcome).toBe('win');
    }
  });
  test('missing stats differ from explicit zero and uncertain zero stats get a brief explanation', () => {
    const m = match({ mode: 'abilitydraftarena' });
    expect(statText(m, 'kills')).toBe('0');
    expect(statsNote(m)).toContain('统计可能不完整');
    m.stats.missing_stats = ['kills'];
    expect(statText(m, 'kills')).toBe('—');
    expect(statText(m, 'deaths')).toBe('0');
    expect(hasKdaStats(m)).toBe(false);
    m.stats.missing_stats = [];
    m.stats.kills = 3;
    expect(statsNote(m)).toBe('');
  });
  test('nonzero equal team scores draw even when the legacy win flag is false', () => {
    expect(resolveMatchResult(score(12, 12))).toMatchObject({ outcome: 'draw', label: '平', score: '12:12' });
    expect(resolveMatchResult(match({ library_metadata: { isDraw: true } })).outcome).toBe('draw');
  });
  test('zero scores never manufacture a draw or display a fabricated score', () => {
    expect(resolveMatchResult(match())).toMatchObject({ outcome: 'unknown', label: '—', score: '', ariaLabel: '暂无可靠对局结果' });
    expect(resolveMatchResult(score(0, 0, { result_known: true, has_won: false }))).toMatchObject({ outcome: 'loss', label: '负', score: '' });
    expect(resolveMatchResult(score(0, 0, { result_known: true, has_won: true }))).toMatchObject({ outcome: 'win', score: '' });
    expect(resolveMatchResult(match({ library_metadata: { hasWon: true } })).outcome).toBe('win');
  });
  test('incomplete and invalid scores stay hidden; valid scores can resolve a missing flag', () => {
    for (const value of [-1, Number.NaN, 1.5, Infinity]) {
      expect(resolveMatchResult(score(value, value, { result_known: false }))).toMatchObject({ outcome: 'unknown', score: '' });
    }
    expect(resolveMatchResult(score(13, 5, { result_known: false })).outcome).toBe('win');
    expect(resolveMatchResult(score(5, 13, { result_known: false })).outcome).toBe('loss');
    expect(resolveMatchResult(score(13, 0, { score_known: false, result_known: false }))).toMatchObject({ outcome: 'unknown', score: '' });
  });
  test('placement modes never infer ties or show round scores, while team deathmatch can draw', () => {
    for (const mode of ['deathmatch', 'abilitydraftarena']) {
      const m = score(12, 12, { result_known: false }, mode);
      m.library_metadata = { isDraw: true };
      expect(resolveMatchResult(m)).toMatchObject({ outcome: 'unknown', score: '' });
      m.library_metadata.hasWon = false;
      expect(resolveMatchResult(m)).toMatchObject({ outcome: 'loss', score: '' });
    }
    expect(resolveMatchResult(score(12, 12, {}, 'teamdeathmatch')).outcome).toBe('draw');
  });
});

describe('account scoped achievement metadata', () => {
  test('same match id across accounts cannot borrow another account award', () => {
    const matches = [match(), match({ openID: 'fixture-b' }), match({ openID: 'fixture-c' })];
    attachMatchMetadata(matches, [
      { openid: 'fixture-a', achievements: [{ matchesId: 'fixture-match', achvType: 'mvp' }], matchDetails: [{ matchesId: 'fixture-match', hasWon: true, achievement: undefined }] },
      { openid: 'fixture-b', matchDetails: [{ matchesId: 'fixture-match', achievement: 'normal' }] },
    ]);
    expect(matches.map(matchAchievement)).toEqual(['mvp', 'normal', 'unknown']);
    expect(resolveMatchResult(matches[0]!).outcome).toBe('win');
    expect(facetValueCounts(matches, EMPTY_FILTERS, 'achievements')).toEqual(new Map([['mvp', 1], ['other', 2]]));
    expect(applyFilters(matches, { ...EMPTY_FILTERS, achievements: ['other'] })).toEqual(matches.slice(1));
  });
  test('fresh facts replace a legacy positive award and stale source annotations are removed', () => {
    const m = match({ library_metadata: { achievement: 'svp' } });
    attachMatchMetadata([m], [{ openid: 'fixture-a', achievements: [{ matchesId: 'fixture-match', achvType: 'mvp' }], matchDetails: [{ matchesId: 'fixture-match', achievement: 'normal' }] }]);
    expect(matchAchievement(m)).toBe('normal');
    attachMatchMetadata([m], []);
    expect(matchAchievement(m)).toBe('unknown');
  });
  test('draw and unknown filters agree with labels and persisted choices', () => {
    const draw = score(12, 12); const unknown = match({ matches_id: 'unknown' });
    expect(applyFilters([draw, unknown], { ...EMPTY_FILTERS, results: ['draw'] })).toEqual([draw]);
    expect(applyFilters([draw, unknown], { ...EMPTY_FILTERS, results: ['unknown'] })).toEqual([unknown]);
    const fs = sanitizePersistedFilters({ results: ['draw', 'unknown', 'bad'], achievements: ['normal', 'unknown', 'mvp', 'bad'] });
    expect(fs.results).toEqual(['draw', 'unknown']);
    expect(fs.achievements).toEqual(['other', 'mvp']);
    expect(activeFilterSummary(fs)).toBe('平 · — · 其他 · MVP');
  });
  test('ability draft Chinese names appear in search and facets', () => {
    const m = match({ mode: 'abilitydraftarena', agent: { agent_name: 'AbilityDraftAgent', agent_id: '' }, map: { map_id: '/Game/Maps/AbilityDraft/AbilityDraft' } });
    expect(resolveMatchAgentLabel(m)).toBe('轮战机器人');
    expect(resolveMatchMapLabel(m)).toBe('轮战竞技场');
    expect(applyFilters([m], { ...EMPTY_FILTERS, query: '轮战机器人' })).toHaveLength(1);
    expect(applyFilters([m], { ...EMPTY_FILTERS, heroes: ['轮战机器人'], maps: ['轮战竞技场'] })).toHaveLength(1);
  });
});
