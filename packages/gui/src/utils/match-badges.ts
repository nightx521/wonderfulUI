import type { MatchRecord } from '@wonderful-ui/parser';
import { VALORANT_RANKS, type ValorantRankMetadata } from './generated/valorant-ranks.zh-CN.ts';

const ranks = new Map(VALORANT_RANKS.map(rank => [rank.tier, rank]));

export interface RankDisplay {
  code: number | null;
  label: string;
  image: string | null;
}

function rankDisplay(value: unknown): RankDisplay {
  // Accept explicit numeric strings, while excluding empty strings and coercible booleans.
  const code = typeof value === 'number' ? value
    : typeof value === 'string' && /^\d+$/.test(value.trim()) ? Number(value.trim()) : NaN;
  const rank: ValorantRankMetadata | undefined = Number.isSafeInteger(code) ? ranks.get(code) : undefined;
  return rank ? { code, label: rank.cn, image: rank.image } : { code: null, label: '未知', image: null };
}

export function matchRank(match: MatchRecord) {
  if (match.mode?.trim().toLowerCase() !== 'competitive') return null;
  // Native serde flattens extras; the CLI reader keeps unknown fields inside extras.
  // Neither current-account rank nor snapshot's default tier zeros are historical evidence.
  const raw = Object.hasOwn(match, 'tier') ? match.tier : match.extras?.tier;
  const tier = raw && typeof raw === 'object' && !Array.isArray(raw)
    ? raw as Record<string, unknown> : {};
  const before = rankDisplay(tier.tier_before);
  const after = rankDisplay(tier.tier_after);
  const changed = before.code !== after.code;
  const entries = changed ? [before, after] : [before];
  return {
    entries,
    label: entries.map(entry => entry.label).join(' → '),
    description: `赛前段位：${before.label}；赛后段位：${after.label}`,
  };
}

const MOMENT_BADGES = [
  { type: '四杀时刻', label: '四杀', tone: 'four', description: '含四杀时刻视频' },
  { type: '五杀时刻', label: '五杀', tone: 'five', description: '含五杀时刻视频' },
] as const;

/** Distinct recorded moment categories, not inferred kills or occurrence counts. */
export function matchMomentBadges(match: MatchRecord) {
  const types = new Set(match.videos.map(video => video.video_type?.trim()));
  return MOMENT_BADGES.filter(badge => types.has(badge.type));
}
