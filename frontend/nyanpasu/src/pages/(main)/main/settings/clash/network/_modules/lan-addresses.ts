import type {
  ClashApiSettings,
  ClashInfo,
  ClashSettings,
  LocalNetwork,
} from '@nyanpasu/rpc/types'

export type LanPortKind = 'controller' | 'mixed' | 'socks5' | 'http'

export interface LanPortEntry {
  kind: LanPortKind
  port: number
}

export interface LanAddressItem {
  kind: LanPortKind
  port: number
  address: string
}

export interface LanAddressGroup {
  interfaceName: string
  ip: string
  network: string
  entries: LanAddressItem[]
}

export interface ResolvedLanPorts {
  ports: LanPortEntry[]
  controllerBindHost: string | null
  controllerLoopbackAddress: string | null
}

const MAX_PORT = 65535

const isValidPort = (port: number | null | undefined): port is number =>
  typeof port === 'number' &&
  Number.isInteger(port) &&
  port > 0 &&
  port <= MAX_PORT

/**
 * Splits a `host:port` or `[ipv6]:port` socket address string.
 * Returns `null` when the string has no valid port in `1..=65535`.
 */
export function parseSocketAddress(
  raw: string | null | undefined,
): { host: string; port: number } | null {
  if (!raw) {
    return null
  }

  const trimmed = raw.trim()
  const separator = trimmed.lastIndexOf(':')
  if (separator <= 0 || separator === trimmed.length - 1) {
    return null
  }

  const rawHost = trimmed.slice(0, separator)
  const host =
    rawHost.startsWith('[') && rawHost.endsWith(']')
      ? rawHost.slice(1, -1)
      : rawHost
  const portText = trimmed.slice(separator + 1)
  if (!host || !/^\d+$/.test(portText)) {
    return null
  }

  const port = Number(portText)
  if (!isValidPort(port)) {
    return null
  }

  return { host, port }
}

const isLoopbackHost = (host: string) => {
  const normalized = host.toLowerCase()
  return (
    normalized === 'localhost' ||
    normalized === '::1' ||
    normalized.startsWith('127.')
  )
}

const isUnspecifiedHost = (host: string) => host === '0.0.0.0' || host === '::'

const formatSocketAddress = (host: string, port: number) =>
  host.includes(':') ? `[${host}]:${port}` : `${host}:${port}`

/**
 * Collects the active controller and proxy ports from the live Clash config,
 * skipping any disabled port (`null`, `undefined`, or `0`).
 *
 * Some cores (such as Mihomo) omit `external-controller` from `GET /configs`;
 * when absent, falls back to the confirmed `ClashInfo` port unless the HTTP
 * controller is explicitly disabled in IPC mode.
 */
export function resolveLiveLanPorts(
  liveConfig: ClashApiSettings | null | undefined,
  fallback?: {
    clashSettings?: ClashSettings | null
    clashInfo?: ClashInfo | null
  },
): ResolvedLanPorts {
  const ports: LanPortEntry[] = []
  let controllerBindHost: string | null = null
  let controllerLoopbackAddress: string | null = null

  const liveController = parseSocketAddress(liveConfig?.['external-controller'])
  if (liveController) {
    ports.push({ kind: 'controller', port: liveController.port })
    controllerBindHost = liveController.host
    if (isLoopbackHost(liveController.host)) {
      controllerLoopbackAddress = formatSocketAddress(
        liveController.host,
        liveController.port,
      )
    }
  } else {
    const httpControllerDisabled =
      fallback?.clashSettings?.clash_control_channel === 'prefer_ipc' &&
      fallback?.clashSettings?.clash_ipc_disable_http_controller === true
    const fallbackController = httpControllerDisabled
      ? null
      : parseSocketAddress(fallback?.clashInfo?.server)

    if (fallbackController) {
      ports.push({ kind: 'controller', port: fallbackController.port })
      const configuredHost =
        fallback?.clashSettings?.external_controller?.host ?? null
      controllerBindHost = configuredHost
      if (configuredHost && isLoopbackHost(configuredHost)) {
        controllerLoopbackAddress = formatSocketAddress(
          configuredHost,
          fallbackController.port,
        )
      }
    }
  }

  if (isValidPort(liveConfig?.['mixed-port'])) {
    ports.push({ kind: 'mixed', port: liveConfig['mixed-port'] })
  }

  if (isValidPort(liveConfig?.['socks-port'])) {
    ports.push({ kind: 'socks5', port: liveConfig['socks-port'] })
  }

  if (isValidPort(liveConfig?.port)) {
    ports.push({ kind: 'http', port: liveConfig.port })
  }

  return {
    ports,
    controllerBindHost,
    controllerLoopbackAddress,
  }
}

export function formatLanAddress(ip: string, entry: LanPortEntry): string {
  switch (entry.kind) {
    case 'controller':
    case 'http':
      return `http://${ip}:${entry.port}`
    case 'socks5':
      return `socks5://${ip}:${entry.port}`
    case 'mixed':
      return `${ip}:${entry.port}`
  }
}

/**
 * Builds the exact LAN addresses for each attached IPv4 network and each
 * enabled port. When the controller is bound to a specific non-loopback LAN IP,
 * it is listed only under that matching interface IP.
 */
export function buildLanAddressGroups(
  networks: readonly LocalNetwork[],
  resolved: ResolvedLanPorts,
): LanAddressGroup[] {
  const specificControllerIp =
    resolved.controllerBindHost &&
    !isUnspecifiedHost(resolved.controllerBindHost) &&
    !isLoopbackHost(resolved.controllerBindHost)
      ? resolved.controllerBindHost
      : null

  return networks.map((item) => {
    const entries: LanAddressItem[] = []

    for (const portEntry of resolved.ports) {
      if (
        portEntry.kind === 'controller' &&
        specificControllerIp &&
        item.address !== specificControllerIp
      ) {
        continue
      }

      entries.push({
        kind: portEntry.kind,
        port: portEntry.port,
        address: formatLanAddress(item.address, portEntry),
      })
    }

    return {
      interfaceName: item.interface,
      ip: item.address,
      network: item.network,
      entries,
    }
  })
}
