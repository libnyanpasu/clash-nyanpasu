import { expect, test } from 'vitest'
import type {
  Proxies_Serialize,
  ProxyGroupItem_Serialize,
  ProxyItem_Serialize,
} from '../../interface/src/ipc/bindings'
import { getGroupSelectedDelay } from '../src/components/proxies/group-delay.ts'

const node = (name: string, delays: number[] = []): ProxyItem_Serialize => ({
  type: 'Direct',
  udp: false,
  provider: null,
  alive: null,
  hidden: false,
  name,
  all: null,
  now: null,
  history: delays.map((delay) => ({ time: '', delay })),
})
const group = (
  name: string,
  now: string | null,
  all: ProxyItem_Serialize[],
): ProxyGroupItem_Serialize => ({ ...node(name), name, now, all })
const snapshot = (
  groups: ProxyGroupItem_Serialize[],
  records: Record<string, ProxyItem_Serialize> = {},
): Proxies_Serialize => ({
  direct: node('DIRECT'),
  proxies: [],
  global: group(
    'GLOBAL',
    groups[0]?.name ?? null,
    groups.map((g) => ({ ...g, all: g.all.map((p) => p.name) })),
  ),
  groups,
  records,
})

test('uses the selected member latest measurement, including failure and no history', () => {
  const selected = node('selected', [30, 80])
  const root = group('root', 'selected', [node('other', [10]), selected])
  const data = snapshot([root])
  expect(getGroupSelectedDelay(root, data)).toBe(80)
  selected.history.push({ time: '', delay: 0 })
  expect(getGroupSelectedDelay(root, data)).toBe(0)
  selected.history = []
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'other'
  expect(getGroupSelectedDelay(root, data)).toBe(10)
})

test('resolves nested and global selections using live members instead of stale records', () => {
  const root = group('root', 'auto', [node('auto', [999])])
  const auto = group('auto', 'leaf', [node('leaf', [42])])
  const data = snapshot([root, auto], { leaf: node('leaf', [900]) })
  expect(getGroupSelectedDelay(root, data)).toBe(42)
  expect(getGroupSelectedDelay(data.global, data)).toBe(42)
  auto.all = [node('leaf', [75])]
  expect(getGroupSelectedDelay(root, data)).toBe(75)
})

test('resolves hidden groups through records and prefers updated member histories', () => {
  const hidden = { ...node('hidden', [999]), all: ['leaf'], now: 'leaf' }
  const root = group('root', 'hidden', [hidden])
  const data = snapshot([root], { hidden, leaf: node('leaf', [42]) })
  expect(getGroupSelectedDelay(root, data)).toBe(42)
  data.groups.push(group('another', 'leaf', [node('leaf', [65])]))
  expect(getGroupSelectedDelay(root, data)).toBe(65)
})

test('missing selections, unresolved nodes, and cycles have no selected latency', () => {
  const root = group('root', null, [])
  const data = snapshot([root])
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'missing'
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'root'
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
  root.now = 'nested'
  data.groups.push(group('nested', 'root', []))
  expect(getGroupSelectedDelay(root, data)).toBe(undefined)
})
