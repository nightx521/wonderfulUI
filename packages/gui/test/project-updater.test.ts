import { describe, expect, test } from 'bun:test';
import { loadUpdateChannel, saveUpdateChannel } from '../src/utils/project-updater.ts';

describe('update channel persistence', () => {
  test('defaults to the proxy and only accepts the official direct option', () => {
    for (const value of [null, '', 'bad', 'https://example.org', 'proxy']) {
      expect(loadUpdateChannel({ getItem: () => value })).toBe('proxy');
    }
    expect(loadUpdateChannel({ getItem: () => 'github' })).toBe('github');
    expect(loadUpdateChannel(null)).toBe('proxy');
  });
  test('remembers a chosen channel and tolerates unavailable storage', () => {
    let saved = '';
    saveUpdateChannel('github', { setItem: (_key, value) => { saved = value; } });
    expect(loadUpdateChannel({ getItem: () => saved })).toBe('github');
    expect(loadUpdateChannel({ getItem: () => { throw Error('blocked'); } })).toBe('proxy');
    expect(() => saveUpdateChannel('proxy', { setItem: () => { throw Error('blocked'); } })).not.toThrow();
  });
});
