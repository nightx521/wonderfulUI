import { describe, expect, test } from 'bun:test';
import {
  SCAN_FAILURE_MESSAGE,
  scanCompletionFeedback,
} from '../src/utils/scan-feedback.ts';

describe('library scan feedback', () => {
  test('distinguishes incremental and full success', () => {
    expect(scanCompletionFeedback('incremental', 0)).toEqual({
      message: '资料库已增量扫描',
      tone: 'ok',
    });
    expect(scanCompletionFeedback('full', 0)).toEqual({
      message: '资料库已全量扫描',
      tone: 'ok',
    });
  });

  test('never reports partial results as unconditional success', () => {
    expect(scanCompletionFeedback('incremental', 2)).toEqual({
      message: '扫描完成，但有 2 个账户或记录读取失败，请查看账户提示或设置中的日志',
      tone: 'error',
    });
    expect(scanCompletionFeedback('full', Number.NaN).tone).toBe('ok');
    expect(SCAN_FAILURE_MESSAGE).not.toContain('\\');
  });

  test('record quarantine is distinct from a complete account read failure', () => {
    expect(scanCompletionFeedback('full', 2, 2).message).toContain('正常记录已更新');
    expect(scanCompletionFeedback('incremental', 3, 2).message).toContain('读取失败');
    expect(scanCompletionFeedback('full', 2, 2).tone).toBe('error');
  });
});
