import { describe, expect, test } from 'vitest'
import type { ClashSettings } from '@nyanpasu/rpc/types'
import {
  buildLanAddressGroups,
  formatLanAddress,
  parseSocketAddress,
  resolveLiveLanPorts,
} from '../src/pages/(main)/main/settings/clash/network/_modules/lan-addresses'

const networks = [
  { interface: 'Ethernet', address: '192.168.1.20', network: '192.168.1.0/24' },
  {
    interface: 'Tailscale',
    address: '100.101.102.103',
    network: '100.64.0.0/10',
  },
]

describe('parseSocketAddress', () => {
  test('parses IPv4 and bracketed IPv6 socket addresses', () => {
    expect(parseSocketAddress('0.0.0.0:9090')).toEqual({
      host: '0.0.0.0',
      port: 9090,
    })
    expect(parseSocketAddress('127.0.0.1:17650')).toEqual({
      host: '127.0.0.1',
      port: 17650,
    })
    expect(parseSocketAddress('[::]:9090')).toEqual({
      host: '::',
      port: 9090,
    })
    expect(parseSocketAddress('[::1]:9091')).toEqual({
      host: '::1',
      port: 9091,
    })
  })

  test('rejects missing, zero, or out-of-range ports', () => {
    for (const invalid of [
      null,
      undefined,
      '',
      '127.0.0.1',
      '127.0.0.1:0',
      '127.0.0.1:65536',
      '127.0.0.1:abc',
      ':9090',
    ]) {
      expect(parseSocketAddress(invalid)).toBeNull()
    }
  })
})

describe('resolveLiveLanPorts', () => {
  test('lists controller, mixed, socks5 and http ports in order and skips disabled ports', () => {
    const resolved = resolveLiveLanPorts({
      'external-controller': '0.0.0.0:9090',
      'mixed-port': 7890,
      'socks-port': 7891,
      port: 7892,
    })

    expect(resolved).toEqual({
      ports: [
        { kind: 'controller', port: 9090 },
        { kind: 'mixed', port: 7890 },
        { kind: 'socks5', port: 7891 },
        { kind: 'http', port: 7892 },
      ],
      controllerBindHost: '0.0.0.0',
      controllerLoopbackAddress: null,
    })

    const partial = resolveLiveLanPorts({
      'external-controller': null,
      'mixed-port': 7890,
      'socks-port': 0,
      port: null,
    })

    expect(partial.ports).toEqual([{ kind: 'mixed', port: 7890 }])
  })

  test('reports loopback binding when controller listens on localhost', () => {
    const resolved = resolveLiveLanPorts({
      'external-controller': '127.0.0.1:9090',
      'mixed-port': 7890,
    })

    expect(resolved.controllerLoopbackAddress).toBe('127.0.0.1:9090')
    expect(resolved.ports).toEqual([
      { kind: 'controller', port: 9090 },
      { kind: 'mixed', port: 7890 },
    ])
  })

  test('falls back to ClashInfo when live config omits external-controller unless HTTP controller is disabled', () => {
    const baseSettings = {
      clash_control_channel: 'prefer_ipc',
      clash_ipc_disable_http_controller: false,
      external_controller: {
        host: '0.0.0.0',
        port: { kind: 'fixed', start_port: 9090 },
      },
    } as ClashSettings

    const enabled = resolveLiveLanPorts(
      { 'external-controller': null, 'mixed-port': 7890 },
      {
        clashSettings: baseSettings,
        clashInfo: { port: 7890, server: '127.0.0.1:19090', secret: null },
      },
    )

    expect(enabled.ports).toEqual([
      { kind: 'controller', port: 19090 },
      { kind: 'mixed', port: 7890 },
    ])
    expect(enabled.controllerLoopbackAddress).toBeNull()

    const disabled = resolveLiveLanPorts(
      { 'external-controller': null, 'mixed-port': 7890 },
      {
        clashSettings: {
          ...baseSettings,
          clash_ipc_disable_http_controller: true,
        },
        clashInfo: { port: 7890, server: '127.0.0.1:19090', secret: null },
      },
    )

    expect(disabled.ports).toEqual([{ kind: 'mixed', port: 7890 }])
  })
})

describe('buildLanAddressGroups', () => {
  test('formats exact addresses for each LAN IP and enabled port', () => {
    const resolved = resolveLiveLanPorts({
      'external-controller': '0.0.0.0:9090',
      'mixed-port': 7890,
      'socks-port': 7891,
      port: 7892,
    })

    expect(buildLanAddressGroups(networks, resolved)).toEqual([
      {
        interfaceName: 'Ethernet',
        ip: '192.168.1.20',
        network: '192.168.1.0/24',
        entries: [
          {
            kind: 'controller',
            port: 9090,
            address: 'http://192.168.1.20:9090',
          },
          { kind: 'mixed', port: 7890, address: '192.168.1.20:7890' },
          { kind: 'socks5', port: 7891, address: 'socks5://192.168.1.20:7891' },
          { kind: 'http', port: 7892, address: 'http://192.168.1.20:7892' },
        ],
      },
      {
        interfaceName: 'Tailscale',
        ip: '100.101.102.103',
        network: '100.64.0.0/10',
        entries: [
          {
            kind: 'controller',
            port: 9090,
            address: 'http://100.101.102.103:9090',
          },
          { kind: 'mixed', port: 7890, address: '100.101.102.103:7890' },
          {
            kind: 'socks5',
            port: 7891,
            address: 'socks5://100.101.102.103:7891',
          },
          { kind: 'http', port: 7892, address: 'http://100.101.102.103:7892' },
        ],
      },
    ])
  })

  test('limits controller address to the bound interface IP when bound to a specific LAN IP', () => {
    const resolved = resolveLiveLanPorts({
      'external-controller': '192.168.1.20:9090',
      'mixed-port': 7890,
    })

    const groups = buildLanAddressGroups(networks, resolved)
    expect(groups[0].entries.map((item) => item.address)).toEqual([
      'http://192.168.1.20:9090',
      '192.168.1.20:7890',
    ])
    expect(groups[1].entries.map((item) => item.address)).toEqual([
      '100.101.102.103:7890',
    ])
  })

  test('formatLanAddress formats every port kind', () => {
    expect(
      formatLanAddress('10.0.0.2', { kind: 'controller', port: 9090 }),
    ).toBe('http://10.0.0.2:9090')
    expect(formatLanAddress('10.0.0.2', { kind: 'mixed', port: 7890 })).toBe(
      '10.0.0.2:7890',
    )
    expect(formatLanAddress('10.0.0.2', { kind: 'socks5', port: 7891 })).toBe(
      'socks5://10.0.0.2:7891',
    )
    expect(formatLanAddress('10.0.0.2', { kind: 'http', port: 7892 })).toBe(
      'http://10.0.0.2:7892',
    )
  })
})
