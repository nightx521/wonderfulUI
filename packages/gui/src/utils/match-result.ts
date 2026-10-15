import type { MatchRecord } from '@wonderful-ui/parser';
import { lookupGameModeAsset } from './valorant-assets.ts';

export type MatchOutcome = 'win' | 'loss' | 'draw' | 'unknown';

export function isPlacementMode(match: MatchRecord): boolean {
  const mode = lookupGameModeAsset(match);
  if (mode?.en === 'Deathmatch' || mode?.en === 'Gauntlet: Glitched') return true;
  return /^(deathmatch|abilitydraftarena)$/i.test(match.mode ?? '');
}

export function resolveMatchResult(match: MatchRecord) {
  const { rounds_won: won, rounds_lost: lost } = match.stats;
  const validScore = match.stats.score_known !== false && Number.isSafeInteger(won) && Number.isSafeInteger(lost)
    && won >= 0 && lost >= 0 && won + lost > 0;
  const placement = isPlacementMode(match);
  const score = validScore && !placement ? `${won}:${lost}` : '';
  const facts = match.library_metadata;
  let outcome: MatchOutcome = 'unknown';
  if (!placement && (facts?.isDraw === true || validScore && won === lost)) {
    outcome = 'draw';
  } else {
    const knownWin = typeof facts?.hasWon === 'boolean' ? facts.hasWon
      : placement ? (match.stats.result_known !== false && match.stats.has_won === true ? true : undefined)
      : match.stats.result_known === true ? match.stats.has_won
      : match.stats.result_known !== false && (match.stats.has_won || validScore) ? match.stats.has_won
      : undefined;
    if (typeof knownWin === 'boolean') outcome = knownWin ? 'win' : 'loss';
    else if (validScore && !placement) outcome = won > lost ? 'win' : 'loss';
  }
  const label = { win: '胜', loss: '负', draw: '平', unknown: '—' }[outcome];
  const description = { win: '胜利', loss: '失败', draw: '平局', unknown: '暂无可靠对局结果' }[outcome];
  return { outcome, label, score, description,
    className: `result-${outcome}`,
    ariaLabel: [description, score].filter(Boolean).join(' '),
  };
}
