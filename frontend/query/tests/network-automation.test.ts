import { expect, test } from 'vitest'
import type {
  NetworkAutomationStatus,
  NetworkAutomationSummary,
} from '@nyanpasu/rpc/types'
import { networkReadingOf } from '../src/ipc/use-network-automation'

const full: NetworkAutomationStatus = {
  supported: true,
  network: {
    state: 'online',
    network: {
      interface_kind: 'wifi',
      wifi: { kind: 'known', ssid: 'home' },
      gateway_mac: 'aa:bb:cc:dd:ee:ff',
    },
  },
  applied: null,
}

test('a full status reply keeps its network reading', () => {
  expect(networkReadingOf(full)).toBe(full)
})

test('a summary reply holds no reading to use', () => {
  const summary: NetworkAutomationSummary = {
    supported: true,
    applied: {
      matched_rule: true,
      outcomes: {
        system_proxy: 'enabled',
        tun: 'unchanged',
        transparent_proxy: 'failed',
      },
    },
  }

  expect(networkReadingOf(summary)).toBeNull()
})

test('an unreadable network is still a full status', () => {
  const offline: NetworkAutomationStatus = {
    supported: true,
    network: null,
    applied: null,
  }

  expect(networkReadingOf(offline)).toBe(offline)
})
