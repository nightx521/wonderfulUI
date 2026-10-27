export interface AccountReadFeedback {
  title: string;
  cause: string;
  action: string;
}

const feedback: Record<string, AccountReadFeedback> = {
  index_conflict: {
    title: '账户记录编号冲突',
    cause: '部分对局或视频编号重复，本次已保留该账户的原有记录。',
    action: '其他账户仍会扫描；稍后刷新重试，详细原因请查看日志。',
  },
  file_missing: {
    title: '账户文件未找到',
    cause: '文件可能已移动，或客户端正在更新数据。',
    action: '确认数据目录仍存在，稍后刷新资料库。',
  },
  access_denied: {
    title: '账户文件无读取权限',
    cause: '当前 Windows 用户可能没有文件访问权限。',
    action: '检查文件夹访问权限，再刷新资料库。',
  },
  file_busy: {
    title: '账户文件暂时无法读取',
    cause: '文件可能被占用，或客户端正在写入数据。',
    action: '等待写入完成后刷新资料库。',
  },
  invalid_file: {
    title: '账户文件内容异常',
    cause: '文件可能尚未写完，或复制的数据不完整。',
    action: '等待写入完成；导入数据请检查原始副本，再重新扫描或导入。',
  },
  decrypt_failed: {
    title: '账户数据无法解密',
    cause: '可能是文件不完整、账户文件被改名或加密格式变化。',
    action: '保留原始数字文件名并检查副本；仍失败请查看设置中的日志。',
  },
  invalid_json: {
    title: '账户数据内容不完整',
    cause: '解密后的内容异常，可能正在写入或已经损坏。',
    action: '等待写入完成后刷新；仍失败请查看设置中的日志。',
  },
  unsupported_format: {
    title: '账户数据格式暂不兼容',
    cause: '高光列表或字段格式异常，可能是数据结构变化。',
    action: '检查应用更新，并在设置中查看日志以便排查。',
  },
  read_failed: {
    title: '账户文件读取失败',
    cause: '可能是磁盘、文件访问或临时读写异常。',
    action: '检查文件可访问后刷新，详细错误请查看设置中的日志。',
  },
};

const unknown: AccountReadFeedback = {
  title: '账户读取失败',
  cause: '尚无法确定原因，可能是文件访问或数据格式异常。',
  action: '刷新资料库以重新检查，详细错误请查看设置中的日志。',
};

/** Raw errors can contain private paths and identifiers; only show curated copy. */
export function accountReadFeedback(account?: { error?: string; errorCode?: string } | null): AccountReadFeedback | null {
  if (!account?.error?.trim()) return null;
  return account.errorCode && Object.hasOwn(feedback, account.errorCode)
    ? feedback[account.errorCode]!
    : unknown;
}
