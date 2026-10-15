import type { MatchRecord } from '@wonderful-ui/parser';
import { isPlacementMode, resolveMatchResult } from './match-result.ts';
type StatKey = 'kills' | 'deaths' | 'assists' | 'score';
const kdaKeys: StatKey[] = ['kills', 'deaths', 'assists'];

export function statText(match: MatchRecord, key: StatKey): string {
  return match.stats.missing_stats?.includes(key) || !Number.isFinite(match.stats[key])
    ? '—' : String(match.stats[key]);
}

export function hasKdaStats(match: MatchRecord): boolean {
  return kdaKeys.every(key => statText(match, key) !== '—');
}

export function statsNote(match: MatchRecord): string {
  if (match.stats.missing_stats?.length) return '部分统计未提供，缺失项显示 —';
  if (isPlacementMode(match) && resolveMatchResult(match).outcome === 'unknown'
    && [...kdaKeys, 'score' as StatKey].every(key => match.stats[key] === 0)) {
    return '统计可能不完整：源记录为全零，未提供可靠结算资料';
  }
  return '';
}
