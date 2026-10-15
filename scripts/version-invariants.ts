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
