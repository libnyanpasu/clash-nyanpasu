import { describe, expect, it } from 'vitest'
import type { ExternalControllerStrategy } from '@nyanpasu/rpc/types'
import {
  formatControllerAddress,
  fromNetworkStatisticWidgetOption,
  parseControllerAddress,
  toNetworkStatisticWidgetOption,
} from '../src/ipc/settings-conversions'

describe('network statistic widget', () => {
  it('maps the tagged config to the flat menu option and back', () => {
    expect(toNetworkStatisticWidgetOption({ kind: 'disabled' })).toBe(
      'disabled',
    )
    expect(
      toNetworkStatisticWidgetOption({ kind: 'enabled', value: 'large' }),
    ).toBe('large')
    expect(fromNetworkStatisticWidgetOption('disabled')).toEqual({
      kind: 'disabled',
    })
    expect(fromNetworkStatisticWidgetOption('small')).toEqual({
      kind: 'enabled',
      value: 'small',
    })
  })
})

describe('external controller address', () => {
  const controller: ExternalControllerStrategy = {
    host: '0.0.0.0',
    port: { kind: 'fixed', start_port: 9090 },
  }

  it('formats and parses host:port, bracketing IPv6 hosts', () => {
    expect(formatControllerAddress(controller)).toBe('0.0.0.0:9090')
    expect(formatControllerAddress({ ...controller, host: '::1' })).toBe(
      '[::1]:9090',
    )
    expect(parseControllerAddress(' 127.0.0.1:9090 ')).toEqual({
      host: '127.0.0.1',
      port: 9090,
    })
    expect(parseControllerAddress('255.255.255.255:65535')).toEqual({
      host: '255.255.255.255',
      port: 65535,
    })
    expect(parseControllerAddress('[::1]:9090')).toEqual({
      host: '::1',
      port: 9090,
    })
  })

  it('round-trips IPv6 hosts through the bracketed form', () => {
    for (const host of ['::', 'fe80::1', '1:2:3:4:5:6:7:8', '::ffff:1.2.3.4']) {
      expect(
        parseControllerAddress(
          formatControllerAddress({ ...controller, host }),
        ),
      ).toEqual({ host, port: 9090 })
    }
  })

  it('rejects an address without a valid port', () => {
    for (const value of ['', '127.0.0.1', ':9090', '127.0.0.1:', 'a:b']) {
      expect(parseControllerAddress(value)).toBeNull()
    }
    expect(parseControllerAddress('127.0.0.1:0')).toBeNull()
    expect(parseControllerAddress('127.0.0.1:65536')).toBeNull()
    expect(parseControllerAddress('[::1]:port')).toBeNull()
  })

  it('rejects a host the backend cannot parse as an IP address', () => {
    for (const value of [
      'localhost:9090',
      '999.999.999.999:9090',
      '256.0.0.1:9090',
      '127.0.0.01:9090',
      '127.0.0:9090',
      '[127.0.0.1]:9090',
      '[localhost]:9090',
      '[1::2::3]:9090',
      '[1:2:3:4:5:6:7:8:9]:9090',
      '[1:2:3:4:5:6:7::8]:9090',
      '[fe80::1%eth0]:9090',
    ]) {
      expect(parseControllerAddress(value)).toBeNull()
    }
  })

  it('rejects an unbracketed IPv6 host as ambiguous with the port', () => {
    for (const value of ['::1', '::1:9090', 'fe80::1:9090']) {
      expect(parseControllerAddress(value)).toBeNull()
    }
  })
})
