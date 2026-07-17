/**
 * Parsing of `install-config` deep links used to import a subscription profile.
 *
 * Supported shapes (percent-encoded `url`, optional `name`):
 *   clash://install-config?url=<http(s) url>&name=<display name>
 *   clash-nyanpasu://install-config?url=<http(s) url>
 *   clash:/install-config?url=...        (single-slash form on some platforms)
 */

export type InstallConfigDeepLink = {
  /** The http(s) subscription URL to import. */
  url: string
  /** Optional user-provided display name, `null` when absent. */
  name: string | null
}

const SUPPORTED_SCHEMES = ['clash:', 'clash-nyanpasu:']

const INSTALL_CONFIG_ACTION = 'install-config'

/**
 * Parse a raw deep link string into an install-config payload.
 * Returns `null` for any unsupported, malformed or non-install-config link.
 */
export function parseInstallConfigDeepLink(
  raw: string,
): InstallConfigDeepLink | null {
  let parsed: URL
  try {
    parsed = new URL(raw)
  } catch {
    return null
  }

  if (!SUPPORTED_SCHEMES.includes(parsed.protocol)) {
    return null
  }

  // Depending on platform and `scheme://` vs `scheme:/` form the action lands
  // either in the host or in the pathname.
  const action = (
    parsed.host || parsed.pathname.replace(/^\/+/, '')
  ).toLowerCase()
  if (action !== INSTALL_CONFIG_ACTION) {
    return null
  }

  // `URLSearchParams` already percent-decodes the values.
  const url = parsed.searchParams.get('url')
  if (!url || !isHttpUrl(url)) {
    return null
  }

  const name = parsed.searchParams.get('name')
  return { url, name: name && name.trim() ? name : null }
}

function isHttpUrl(value: string): boolean {
  try {
    const { protocol } = new URL(value)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}
