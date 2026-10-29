export function nextReleaseVersion(current: string, bump: 'patch' | 'minor' | 'major'): string {
  const parts = /^(\d+)\.(\d+)\.(\d+)(-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.exec(current);
  if (!parts) throw new Error('Invalid current semantic version');
  const major = Number(parts[1]), minor = Number(parts[2]), patch = Number(parts[3]);
  if (bump === 'major') return `${major + 1}.0.0`;
  if (bump === 'minor') return `${major}.${minor + 1}.0`;
  return parts[4] ? `${major}.${minor}.${patch}` : `${major}.${minor}.${patch + 1}`;
}

export function releaseTagMismatch(
  version: string,
  refType: string | undefined,
  refName: string | undefined,
  prerelease = false,
): string | undefined {
  if (refType !== 'tag') return undefined;

  const expectedTag = `v${version}`;
  if (refName === expectedTag) return undefined;
  const previewPrefix = `${expectedTag}-pre.`;
  if (prerelease && refName?.startsWith(previewPrefix) && /^[1-9]\d*$/.test(refName.slice(previewPrefix.length))) return undefined;
  return `release tag must be ${expectedTag}, got ${refName || '(empty)'}`;
}
