import { expect, test } from 'bun:test';
import { createCipheriv } from 'node:crypto';
import { deriveKeyIv } from '../src/crypto.ts';
import { parseWonderfulDbBuffer, parseSnapshotDbBuffer } from '../src/reader.ts';

async function encrypted(value: unknown, bom = false): Promise<Uint8Array> {
  const { key, iv } = await deriveKeyIv('123');
  const cipher = createCipheriv('aes-256-cbc', key, iv);
  const bytes = Buffer.concat([cipher.update(JSON.stringify(value)), cipher.final()]);
  return Buffer.from((bom ? '\uFEFF' : '') + bytes.toString('hex'));
}

test('nullable statistics remain missing and nullable collections are empty', async () => {
  const parsed = await parseWonderfulDbBuffer(await encrypted({ key_wonderful_list_123: [
    { map: null, agent: null, stats: { kills: null, has_won: null, rounds_won: null }, videos: null },
    { stats: null, videos: [{ rounds: null }, { rounds: [{ round_clips: null, round_honors: null }] }] },
  ] }), '123');
  expect(parsed.matches[0]!.stats.result_known).toBe(false);
  expect(parsed.matches[0]!.stats.missing_stats).toEqual(['kills', 'deaths', 'assists', 'score']);
  expect(parsed.matches[0]!.videos).toEqual([]);
  expect(parsed.matches[1]!.videos[0]!.rounds ?? []).toEqual([]);
});

test('unknown objects and invalid collection types do not become empty libraries', async () => {
  for (const payload of [{ future_field: [] }, { key_wonderful_list_123: null }, { key_wonderful_list_123: [{ videos: {} }] }]) {
    await expect(parseWonderfulDbBuffer(await encrypted(payload), '123')).rejects.toThrow();
  }
  for (const payload of [{}, { key_wonderful_list_123: [] }]) {
    expect((await parseWonderfulDbBuffer(await encrypted(payload), '123')).matches).toEqual([]);
  }
});

test('BOM and numeric-string snapshot timestamps are compatible', async () => {
  expect((await parseWonderfulDbBuffer(await encrypted({ key_wonderful_list_123: [] }, true), '123')).matches).toEqual([]);
  const snapshot = await parseSnapshotDbBuffer(await encrypted({ key_snapshot_list123: [
    { matches_time: '2', matches_id: 'fixture', snapshot: { ss_nick: 'NewFixture', ss_achieve_type: 'mvp' } },
    { matches_time: 1, snapshot: { ss_nick: 'OldFixture' } },
  ] }), '123');
  expect(snapshot.nick).toBe('NewFixture');
  expect(snapshot.achievements?.[0]?.typeStr).toBe('MVP');
});
