import type { MatchRecord, SnapshotMatchDetail } from '@wonderful-ui/parser';

interface AccountFacts {
  openid: string;
  achievements?: { matchesId: string; achvType: string }[];
  matchDetails?: SnapshotMatchDetail[];
}

/** Account + match is the identity boundary, including imported match aliases. */
export function attachMatchMetadata(matches: MatchRecord[], accounts: AccountFacts[]): void {
  const byAccount = new Map<string, Map<string, Omit<SnapshotMatchDetail, 'matchesId'>>>();
  for (const account of accounts) {
    const facts = new Map<string, Omit<SnapshotMatchDetail, 'matchesId'>>();
    for (const item of account.achievements ?? []) {
      if (item.achvType === 'mvp' || item.achvType === 'svp') {
        facts.set(item.matchesId, { achievement: item.achvType });
      }
    }
    for (const { matchesId, ...detail } of account.matchDetails ?? []) {
      const merged = { ...facts.get(matchesId) };
      if (detail.achievement !== undefined) merged.achievement = detail.achievement;
      if (typeof detail.isDraw === 'boolean') merged.isDraw = detail.isDraw;
      if (typeof detail.hasWon === 'boolean') merged.hasWon = detail.hasWon;
      facts.set(matchesId, merged);
    }
    byAccount.set(account.openid, facts);
  }
  for (const match of matches) match.library_metadata = byAccount.get(match.openID)?.get(match.matches_id);
}

export function matchAchievement(match: MatchRecord): 'mvp' | 'svp' | 'normal' | 'unknown' {
  const type = match.library_metadata?.achievement;
  return type === 'mvp' || type === 'svp' || type === 'normal' ? type : 'unknown';
}

/** Filter categories group all records without an explicit MVP/SVP together. */
export function matchAchievementCategory(match: MatchRecord): 'mvp' | 'svp' | 'other' {
  const achievement = matchAchievement(match);
  return achievement === 'mvp' || achievement === 'svp' ? achievement : 'other';
}

/** Room classification follows the primary ACLOS mode marker. */
export function matchRoomType(match: MatchRecord): 'custom' | 'non-custom' {
  return match.mode?.trim().toLowerCase() === 'customgame' ? 'custom' : 'non-custom';
}
