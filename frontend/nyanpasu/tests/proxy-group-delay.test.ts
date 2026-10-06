import { expect, test } from 'vitest'
import type {
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'
import { getGroupSelectedDelay } from '../src/components/proxies/group-delay.ts'

const node = (name: string, delays: number[] = []): Proxy_Serialize => ({
  name,
  type: 'Direct',
  udp: false,
  history: delays.map((delay) => ({ time: '', delay })),
  id: null,
  now: null,
  all: null,
  testUrl: null,
  expectedStatus: null,
  fixed: null,
  hidden: null,
  icon: null,
  emptyFallback: null,
  provider: null,
})
// A group's record, as it appears among the nodes.
const groupRecord = (
  name: string,
  now: string | null,
  all: string[],
): Proxy_Serialize => ({ ...node(name), type: 'Selector', now, all })
const group = (
  name: string,
  now: string | null,
  all: string[],
): ProxyGroup => ({
  name,
  type: 'Selector',
  all,
  now,
  fixed: null,
  testUrl: null,
  expectedStatus: null,
  hidden: false,
  icon: null,
  capabilities: { select: true, clearFixed: false },
})
const snapshot = (
  global: ProxyGroup | null,
  groups: ProxyGroup[],
  nodes: Record<string, Proxy_Serialize>,
): Proxies_Serialize => ({ global, groups, nodes })

test('uses the selected member latest measurement, including failure and no history', () => {
  const other = node('other', [10])
  const selected = node('selected', [30, 80])
  const root = group('root', 'selected', ['other', 'selected'])
  const nodes = { other, selected }
  const data = snapshot(root, [root], nodes)
  expect(getGroupSelectedDelay(root, data)).toBe(80)
  selected.history.push({ time: '', delay: 0 })
  expect(getGroupSelectedDelay(root, data)).toBe(0)
  selected.history = []
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'other'
  expect(getGroupSelectedDelay(root, data)).toBe(10)
})

test('resolves nested and global selections through the shared node record', () => {
  const leaf = node('leaf', [42])
  const auto = groupRecord('auto', 'leaf', ['leaf'])
  const root = group('root', 'auto', ['auto'])
  const nodes = { auto, leaf }
  const data = snapshot(root, [root, group('auto', 'leaf', ['leaf'])], nodes)
  expect(getGroupSelectedDelay(root, data)).toBe(42)
  expect(getGroupSelectedDelay(data.global!, data)).toBe(42)
  // Only one `leaf` object exists, so a new sample is visible from every path.
  leaf.history.push({ time: '', delay: 75 })
  expect(getGroupSelectedDelay(root, data)).toBe(75)
})

test('resolves a hidden group that is not listed at the top level', () => {
  const leaf = node('leaf', [42])
  const hidden = groupRecord('hidden', 'leaf', ['leaf'])
  const root = group('root', 'hidden', ['hidden'])
  const nodes = { hidden, leaf }
  const data = snapshot(root, [root], nodes)
  expect(getGroupSelectedDelay(root, data)).toBe(42)
})

test('missing selections, unresolved nodes, and cycles have no selected latency', () => {
  const root = group('root', null, [])
  const data = snapshot(root, [root], {})
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'missing'
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'root'
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  data.groups.push(group('nested', 'root', ['root']))
  data.nodes.nested = groupRecord('nested', 'root', ['root'])
  root.now = 'nested'
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
})
