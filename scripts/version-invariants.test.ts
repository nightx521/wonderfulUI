import { describe, expect, test } from 'bun:test';
import { releaseTagMismatch } from './version-invariants.ts';

describe('releaseTagMismatch', () => {
  test('accepts numbered preview tags only for a declared preview of the same version', () => {
    expect(releaseTagMismatch('1.2.3', 'tag', 'v1.2.3-pre.1', true)).toBeUndefined();
    expect(releaseTagMismatch('1.2.3', 'tag', 'v1.2.3-pre.2', true)).toBeUndefined();
    expect(releaseTagMismatch('1.2.3', 'tag', 'v1.2.3-pre.1')).toBeDefined();
    expect(releaseTagMismatch('1.2.3', 'tag', 'v1.2.2-pre.1', true)).toBeDefined();
    expect(releaseTagMismatch('1.2.3', 'tag', 'v1.2.3-pre.0', true)).toBeDefined();
    expect(releaseTagMismatch('1.2.3', 'tag', 'v1.2.3-pre.1-extra', true)).toBeDefined();
  });
  test('accepts the tag derived from the canonical version', () => {
    expect(releaseTagMismatch('1.2.3', 'tag', 'v1.2.3')).toBeUndefined();
  });

  test('rejects a tag that would publish a mismatched release version', () => {
    expect(releaseTagMismatch('1.2.3', 'tag', 'v1.2.2')).toBe(
      'release tag must be v1.2.3, got v1.2.2',
    );
  });

  test('does not impose a tag on local, branch, or manual checks', () => {
    expect(releaseTagMismatch('1.2.3', undefined, undefined)).toBeUndefined();
    expect(releaseTagMismatch('1.2.3', 'branch', 'main')).toBeUndefined();
  });
});
