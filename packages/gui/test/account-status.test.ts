import { describe, expect, test } from 'bun:test';
import { accountReadFeedback } from '../src/utils/account-status.ts';

describe('account read feedback', () => {
  test('empty data is not an error', () => {
    expect(accountReadFeedback()).toBeNull();
    expect(accountReadFeedback({ error: ' ', errorCode: 'invalid_file' })).toBeNull();
  });

  test('common failures have a cause and a recovery hint without exposing raw text', () => {
    for (const code of ['file_missing', 'access_denied', 'file_busy', 'invalid_file',
      'decrypt_failed', 'invalid_json', 'unsupported_format', 'read_failed']) {
      const result = accountReadFeedback({ error: 'D:\\private\\test-account', errorCode: code });
      expect(result?.title).toBeTruthy();
      expect(result?.cause).toBeTruthy();
      expect(result?.action).toBeTruthy();
      expect(JSON.stringify(result)).not.toContain('private');
      expect(JSON.stringify(result)).not.toContain('test-account');
    }
    expect(accountReadFeedback({ error: 'test', errorCode: 'file_busy' })?.cause).toContain('占用');
    expect(accountReadFeedback({ error: 'test', errorCode: 'access_denied' })?.cause).toContain('权限');
  });

  test('legacy and unknown codes have a safe generic explanation', () => {
    for (const errorCode of [undefined, 'future_code', '__proto__', 'constructor']) {
      expect(accountReadFeedback({ error: 'private details', errorCode })?.title).toBe('账户读取失败');
    }
  });
});
