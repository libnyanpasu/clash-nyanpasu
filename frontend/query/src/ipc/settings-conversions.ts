import type {
  ExternalControllerStrategy,
  NetworkStatisticWidgetConfig,
  StatisticWidgetVariant,
} from '@nyanpasu/rpc/types'

export type NetworkStatisticWidgetOption = 'disabled' | StatisticWidgetVariant

export const toNetworkStatisticWidgetOption = (
  config: NetworkStatisticWidgetConfig,
): NetworkStatisticWidgetOption =>
  config.kind === 'enabled' ? config.value : 'disabled'

export const fromNetworkStatisticWidgetOption = (
  option: NetworkStatisticWidgetOption,
): NetworkStatisticWidgetConfig =>
  option === 'disabled'
    ? { kind: 'disabled' }
    : { kind: 'enabled', value: option }

export type ControllerAddress = {
  host: string
  port: number
}

/** A dotted-quad IPv4 literal without leading zeros, as `Ipv4Addr` parses. */
const IPV4_LITERAL =
  /^(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$/

const IPV6_GROUP = /^[\da-f]{1,4}$/i

/**
 * An IPv6 literal as `Ipv6Addr` parses it: eight groups, or fewer around one
 * `::` that stands for at least one zero group, where an IPv4 literal may take
 * the place of the last two groups. No zone index.
 */
const isIpv6Literal = (host: string) => {
  const halves = host.split('::')

  if (halves.length > 2) {
    return false
  }

  const groups = halves.map((half) => (half ? half.split(':') : []))
  const tail = groups[groups.length - 1]
  const ipv4Tail = tail.length > 0 && IPV4_LITERAL.test(tail[tail.length - 1])
  const hexGroups = groups.flat().slice(0, ipv4Tail ? -1 : undefined)
  const width = hexGroups.length + (ipv4Tail ? 2 : 0)

  return (
    hexGroups.every((group) => IPV6_GROUP.test(group)) &&
    (halves.length === 2 ? width < 8 : width === 8)
  )
}

/**
 * `host:port`, where the host is an IP literal the backend's `IpAddr` accepts:
 * IPv4, or IPv6 in brackets. Hostnames are rejected, and so is an unbracketed
 * IPv6 host, whose last group cannot be told apart from the port.
 */
export const parseControllerAddress = (
  value: string,
): ControllerAddress | null => {
  const trimmed = value.trim()
  const separator = trimmed.lastIndexOf(':')

  if (separator <= 0) {
    return null
  }

  const hostText = trimmed.slice(0, separator)
  const bracketed = /^\[(.*)\]$/.exec(hostText)
  const host = bracketed ? bracketed[1] : hostText
  const portText = trimmed.slice(separator + 1)

  if (
    !(bracketed ? isIpv6Literal(host) : IPV4_LITERAL.test(host)) ||
    !/^\d+$/.test(portText)
  ) {
    return null
  }

  const port = Number(portText)

  return port >= 1 && port <= 65535 ? { host, port } : null
}

export const formatControllerAddress = (
  controller: ExternalControllerStrategy,
): string => {
  const host = controller.host.includes(':')
    ? `[${controller.host}]`
    : controller.host

  return `${host}:${controller.port.start_port}`
}
