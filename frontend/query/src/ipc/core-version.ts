import { gt, parse, valid } from 'semver'

const rollingCoreNames = new Set([
  'mihomo-alpha',
  'clash-rs-alpha',
  'meow-alpha',
])

const parseVersion = (raw?: string) => {
  if (!raw || raw === 'N/A') return undefined
  const normalized = raw.startsWith('v') ? raw.slice(1) : raw
  return valid(normalized) ? (parse(normalized) ?? undefined) : undefined
}

const parsePremiumVersion = (raw?: string) => {
  if (!raw || raw === 'N/A') return undefined
  const match = /^(?:n)?(\d{4}-\d{2}-\d{2})(?:-g([\da-f]{7,40}))?$/i.exec(raw)
  return match ? { date: match[1], hash: match[2]?.toLowerCase() } : undefined
}

const rollingHash = (raw: string, build: readonly string[] = []) => {
  const hash =
    /^sha\.([\da-f]{7,40})$/i.exec(build.join('.'))?.[1] ??
    (/^[\da-f]{7,40}$/i.test(build.join('.')) ? build.join('.') : undefined) ??
    /(?:^|[-+])alpha-([\da-f]{7,40})(?:$|[.+-])/i.exec(raw)?.[1]
  return hash?.toLowerCase()
}

export function hasNewerCoreVersion(
  core: string | undefined,
  currentVersion?: string,
  latestVersion?: string,
): boolean {
  if (!core || !currentVersion || !latestVersion || currentVersion === 'N/A') {
    return false
  }

  if (core === 'clash') {
    const current = parsePremiumVersion(currentVersion)
    const latest = parsePremiumVersion(latestVersion)
    if (!current || !latest) return false
    if (latest.date !== current.date) return latest.date > current.date
    return Boolean(current.hash && latest.hash && current.hash !== latest.hash)
  }

  const current = parseVersion(currentVersion)
  const latest = parseVersion(latestVersion)

  if (rollingCoreNames.has(core)) {
    const currentHash = rollingHash(currentVersion, current?.build)
    const latestHash = rollingHash(latestVersion, latest?.build)
    if (currentHash && latestHash) return currentHash !== latestHash
  }

  if (!current || !latest) return false

  if (rollingCoreNames.has(core)) {
    if (
      current.compare(latest) === 0 &&
      current.build.join('.') !== latest.build.join('.')
    ) {
      return current.build.length > 0 && latest.build.length > 0
    }
  }

  return gt(latest, current)
}

export function canUpdateCoreVersion(
  core: string | undefined,
  currentVersion?: string,
  latestVersion?: string,
  versionReadError = false,
): boolean {
  if (hasNewerCoreVersion(core, currentVersion, latestVersion)) return true
  if (!versionReadError || !core || !latestVersion || latestVersion === 'N/A') {
    return false
  }
  if (core === 'clash') return parsePremiumVersion(latestVersion) !== undefined
  if (rollingCoreNames.has(core)) {
    return (
      parseVersion(latestVersion) !== undefined ||
      rollingHash(latestVersion) !== undefined
    )
  }
  return parseVersion(latestVersion) !== undefined
}
