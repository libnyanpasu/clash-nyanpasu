import { expect, test } from 'vitest'
import type {
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'
import {
  collectDelayKeys,
  DEFAULT_DELAY_KEY,
  groupDelayKey,
  latestDelay,
  memberDelay,
  memberState,
  nodeDelayHistory,
  resolveChain,
} from '../src/ipc/proxy-delay'

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
  delayKey = DEFAULT_DELAY_KEY,
): ProxyGroup => ({
  name,
  type: 'Selector',
  all,
  now,
  fixed: null,
  delayKey,
  expectedStatus: null,
  hidden: false,
  iconConfigured: false,
  capabilities: { select: true, clearFixed: false },
})
const snapshot = (
  global: ProxyGroup | null,
  groups: ProxyGroup[],
  nodes: Record<string, Proxy_Serialize>,
): Proxies_Serialize => ({ global, groups, nodes })

const sample = (delay: number, time = '') => ({
  alive: delay > 0,
  history: [{ time, delay }],
})

test('a delay source is named by its key and never by a URL', () => {
  const base = node('n', [10, 20])
  const withExtra = {
    ...base,
    extra: { '': sample(15), 'Group A': sample(99) },
  }
  expect(latestDelay(withExtra, 'Group A')).toBe(99)
  expect(latestDelay(withExtra, DEFAULT_DELAY_KEY)).toBe(15)
  expect(latestDelay(withExtra, 'Group B')).toBe(20) // never tested for it
  expect(latestDelay({ ...base, extra: {} }, 'Group A')).toBe(20) // Clash-rs
  expect(latestDelay(base, 'Group A')).toBe(20) // Meow: no extra
  expect(latestDelay(node('empty'), 'Group A')).toBeUndefined()
  expect(latestDelay(undefined, 'Group A')).toBeUndefined()
  expect(nodeDelayHistory(withExtra, 'Group A')).toEqual([
    { time: '', delay: 99 },
  ])
})

test('a group is looked up by name, else it tests with the default', () => {
  const own = group('own', null, [], 'own')
  const plain = group('plain', null, [])
  const data = snapshot(plain, [own, plain], {})
  expect(groupDelayKey(data, 'own')).toBe('own')
  expect(groupDelayKey(data, 'plain')).toBe(DEFAULT_DELAY_KEY)
  expect(groupDelayKey(data, 'missing')).toBe(DEFAULT_DELAY_KEY)
  expect(groupDelayKey(data, undefined)).toBe(DEFAULT_DELAY_KEY)
  expect(groupDelayKey(undefined, 'own')).toBe(DEFAULT_DELAY_KEY)
  // A name that is also an Object property is not a group.
  expect(groupDelayKey(data, 'toString')).toBe(DEFAULT_DELAY_KEY)
  expect(collectDelayKeys(data)).toEqual({ own: 'own', plain: '' })
})

test('a hidden group still has its key once it left the group list', () => {
  const hidden = group('hidden', null, [], 'hidden')
  const data = {
    ...snapshot(null, [], {}),
    delayKeys: collectDelayKeys(snapshot(null, [hidden], {})),
  }
  expect(groupDelayKey(data, 'hidden')).toBe('hidden')
})

test('a chain follows each nested selection to its leaf', () => {
  const proxies = snapshot(null, [], {
    outer: groupRecord('outer', 'inner', ['inner']),
    inner: groupRecord('inner', 'leaf', ['leaf']),
    leaf: node('leaf', [30]),
  })
  expect(resolveChain('outer', proxies)).toEqual({
    path: ['inner', 'leaf'],
    leaf: proxies.nodes.leaf,
    parent: 'inner',
  })
})

test('a chain stops at a cycle or a missing node', () => {
  const cycle = snapshot(null, [], {
    a: groupRecord('a', 'b', ['b']),
    b: groupRecord('b', 'a', ['a']),
  })
  expect(resolveChain('a', cycle).leaf).toBeUndefined()
  const missing = snapshot(null, [], { a: groupRecord('a', 'gone', ['gone']) })
  expect(resolveChain('a', missing).leaf).toBeUndefined()
  const unselected = snapshot(null, [], { a: groupRecord('a', null, []) })
  expect(resolveChain('a', unselected)).toEqual({ path: [], parent: 'a' })
})

test("a member that is a group reports its leaf under the leaf's group key", () => {
  const inner = group('inner', 'leaf', ['leaf'], 'inner')
  const leaf = {
    ...node('leaf', [70]),
    extra: { inner: sample(40), '': sample(99) },
  }
  const outer = group('outer', 'inner', ['inner'], 'outer')
  const proxies = snapshot(null, [outer, inner], {
    inner: groupRecord('inner', 'leaf', ['leaf']),
    leaf,
    plain: node('plain', [5]),
  })
  expect(memberState('inner', outer, proxies).delay).toBe(40)
  expect(memberState('plain', outer, proxies).delay).toBe(5)
  expect(memberState('gone', outer, proxies).delay).toBeUndefined()
})

test('a hidden nested group is read under its own key', () => {
  const hidden = group('hidden', 'leaf', ['leaf'], 'hidden')
  const outer = group('outer', 'hidden', ['hidden'])
  const full = snapshot(null, [outer, hidden], {
    hidden: groupRecord('hidden', 'leaf', ['leaf']),
    leaf: { ...node('leaf', [70]), extra: { hidden: sample(40) } },
  })
  const listed = {
    ...full,
    groups: [outer],
    delayKeys: collectDelayKeys(full),
  }
  expect(memberState('hidden', outer, listed).delay).toBe(40)
})

test('groups sharing a node each read the history of their own source', () => {
  const shared = {
    ...node('shared', [1]),
    extra: { '': sample(10), a: sample(20), b: sample(30) },
  }
  const defaults = group('defaults', 'shared', ['shared'])
  const a = group('a', 'shared', ['shared'], 'a')
  const b = group('b', 'shared', ['shared'], 'b')
  const data = snapshot(null, [defaults, a, b], { shared })
  expect(memberDelay('shared', defaults, data)).toBe(10)
  expect(memberDelay('shared', a, data)).toBe(20)
  expect(memberDelay('shared', b, data)).toBe(30)
})

// Cases ported from the sidebar's former group-delay tests.
test('uses the selected member latest measurement, including failure and no history', () => {
  const other = node('other', [10])
  const selected = node('selected', [30, 80])
  const root = group('root', 'selected', ['other', 'selected'])
  const data = snapshot(root, [root], { other, selected })
  const now = () => memberState(root.now!, root, data).delay
  expect(now()).toBe(80)
  selected.history.push({ time: '', delay: 0 })
  expect(now()).toBe(0)
  selected.history = []
  expect(now()).toBe(undefined)
  root.now = 'other'
  expect(now()).toBe(10)
})

test('resolves nested and global selections through the shared node record', () => {
  const leaf = node('leaf', [42])
  const auto = groupRecord('auto', 'leaf', ['leaf'])
  const root = group('root', 'auto', ['auto'])
  const data = snapshot(root, [root, group('auto', 'leaf', ['leaf'])], {
    auto,
    leaf,
  })
  expect(memberState('auto', root, data).delay).toBe(42)
  expect(memberState(data.global!.now!, data.global!, data).delay).toBe(42)
  // Only one `leaf` object exists, so a new sample is visible from every path.
  leaf.history.push({ time: '', delay: 75 })
  expect(memberState('auto', root, data).delay).toBe(75)
})

test('resolves a hidden group that is not listed at the top level', () => {
  const leaf = node('leaf', [42])
  const hidden = groupRecord('hidden', 'leaf', ['leaf'])
  const root = group('root', 'hidden', ['hidden'])
  const data = snapshot(root, [root], { hidden, leaf })
  expect(memberState('hidden', root, data).delay).toBe(42)
})

test('missing selections, unresolved nodes, and cycles have no delay', () => {
  const root = group('root', 'missing', [])
  const data = snapshot(root, [root], {})
  expect(memberState('missing', root, data).delay).toBe(undefined)
  data.nodes.root = groupRecord('root', 'nested', ['nested'])
  data.nodes.nested = groupRecord('nested', 'root', ['root'])
  expect(memberState('nested', root, data).delay).toBe(undefined)
  expect(memberState('root', root, data).delay).toBe(undefined)
})

test('a nested member carries the history of the leaf its delay comes from', () => {
  const leaf = node('leaf', [99, 42])
  const inner = group('inner', 'leaf', ['leaf'], 'inner')
  const innerHistory = [{ time: '', delay: 42 }]
  leaf.extra = {
    inner: { alive: true, history: innerHistory },
    '': sample(99),
  }
  const root = group('root', 'inner', ['inner'])
  const data = snapshot(root, [root, inner], {
    inner: groupRecord('inner', 'leaf', ['leaf']),
    leaf,
  })
  const state = memberState('inner', root, data)
  expect(state.leaf).toBe('leaf')
  expect(state.delay).toBe(42)
  expect(state.history).toBe(innerHistory)
})

test('a plain member carries its own history under the group source', () => {
  const plain = node('plain', [7])
  const root = group('root', 'plain', ['plain'])
  const data = snapshot(root, [root], { plain })
  const state = memberState('plain', root, data)
  expect(state).toEqual({ delay: 7, history: plain.history })
  expect(state.history).toBe(plain.history)
})

test('memberDelay is the delay of the member state', () => {
  const plain = node('plain', [7])
  const root = group('root', 'plain', ['plain'])
  const data = snapshot(root, [root], { plain })
  expect(memberDelay('plain', root, data)).toBe(7)
})
