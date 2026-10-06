import { expect, test, vi } from 'vitest'
import type {
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'
import {
  groupTestUrl,
  latestDelay,
  memberDelay,
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

const D = 'https://d/'

test('a group tests against its own URL, else the default', () => {
  expect(groupTestUrl({ testUrl: 'https://a/' }, 'https://d/')).toBe(
    'https://a/',
  )
  expect(groupTestUrl({ testUrl: '' }, 'https://d/')).toBe('https://d/')
  expect(groupTestUrl({ testUrl: null }, 'https://d/')).toBe('https://d/')
  expect(groupTestUrl(undefined, 'https://d/')).toBe('https://d/')
})

test('delays come from extra[url], else history', () => {
  const base = node('n', [10, 20])
  const withExtra = {
    ...base,
    extra: {
      'https://a/': { alive: true, history: [{ time: '', delay: 99 }] },
    },
  }
  expect(latestDelay(withExtra, 'https://a/')).toBe(99)
  expect(latestDelay(withExtra, 'https://b/')).toBe(20) // URL never tested here
  expect(latestDelay({ ...base, extra: {} }, 'https://a/')).toBe(20) // Clash-rs
  expect(latestDelay(base, 'https://a/')).toBe(20) // Meow: no extra
  expect(latestDelay(node('empty'), 'https://a/')).toBeUndefined()
  expect(latestDelay(undefined, 'https://a/')).toBeUndefined()
  expect(nodeDelayHistory(withExtra, 'https://a/')).toEqual([
    { time: '', delay: 99 },
  ])
})

test('delays read both the raw and the normalized URL key, newest wins', () => {
  const sample = (time: string, delay: number) => ({
    alive: true,
    history: [{ time, delay }],
  })
  const base = node('n', [10])

  // Only the backend's normalized key exists for a bare-host group URL.
  const slashOnly = {
    ...base,
    extra: { 'https://cp.cloudflare.com/': sample('2026-10-06T00:00:00Z', 55) },
  }
  expect(latestDelay(slashOnly, 'https://cp.cloudflare.com')).toBe(55)

  const both = (rawTime: string, normalizedTime: string) => ({
    ...base,
    extra: {
      'https://cp.cloudflare.com': sample(rawTime, 11),
      'https://cp.cloudflare.com/': sample(normalizedTime, 22),
    },
  })
  const earlier = '2026-10-06T00:00:00Z'
  const later = '2026-10-06T00:00:05Z'
  expect(latestDelay(both(earlier, later), 'https://cp.cloudflare.com')).toBe(
    22,
  )
  expect(latestDelay(both(later, earlier), 'https://cp.cloudflare.com')).toBe(
    11,
  )
  expect(latestDelay(both(later, later), 'https://cp.cloudflare.com')).toBe(11)

  // An unparsable URL is only ever looked up verbatim.
  const odd = { ...base, extra: { 'https://[': sample(earlier, 7) } }
  expect(latestDelay(odd, 'https://[')).toBe(7)
  expect(latestDelay(odd, 'not a url')).toBe(10)
})

test('delays still resolve where URL.canParse is unavailable', () => {
  vi.stubGlobal('URL', class extends URL {})
  // @ts-expect-error simulate WebKit before 17
  URL.canParse = undefined
  try {
    const sample = (delay: number) => ({
      alive: true,
      history: [{ time: '2026-10-06T00:00:00Z', delay }],
    })
    const base = node('n', [10])
    const slashOnly = {
      ...base,
      extra: { 'https://cp.cloudflare.com/': sample(55) },
    }
    expect(latestDelay(slashOnly, 'https://cp.cloudflare.com')).toBe(55)
    expect(latestDelay(slashOnly, 'https://cp.cloudflare.com/')).toBe(55)
    const raw = { ...base, extra: { 'https://[': sample(7) } }
    expect(latestDelay(raw, 'https://[')).toBe(7)
    expect(latestDelay(raw, 'not a url')).toBe(10)
  } finally {
    vi.unstubAllGlobals()
  }
})

test('a chain follows each nested selection to its leaf', () => {
  const proxies = snapshot(null, [], {
    outer: groupRecord('outer', 'inner', ['inner']),
    inner: { ...groupRecord('inner', 'leaf', ['leaf']), testUrl: 'https://i/' },
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

test("a member that is a group reports its leaf under the leaf's group URL", () => {
  const inner = {
    ...groupRecord('inner', 'leaf', ['leaf']),
    testUrl: 'https://i/',
  }
  const leaf = {
    ...node('leaf', [70]),
    extra: {
      'https://i/': { alive: true, history: [{ time: '', delay: 40 }] },
    },
  }
  const proxies = snapshot(null, [], { inner, leaf, plain: node('plain', [5]) })
  const outer = { testUrl: 'https://o/' }
  expect(memberDelay('inner', outer, proxies, 'https://d/')).toBe(40)
  expect(memberDelay('plain', outer, proxies, 'https://d/')).toBe(5)
  expect(memberDelay('gone', outer, proxies, 'https://d/')).toBeUndefined()
})

// Cases ported from the sidebar's former group-delay tests.
test('uses the selected member latest measurement, including failure and no history', () => {
  const other = node('other', [10])
  const selected = node('selected', [30, 80])
  const root = group('root', 'selected', ['other', 'selected'])
  const data = snapshot(root, [root], { other, selected })
  const now = () => memberDelay(root.now!, root, data, D)
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
  expect(memberDelay('auto', root, data, D)).toBe(42)
  expect(memberDelay(data.global!.now!, data.global!, data, D)).toBe(42)
  // Only one `leaf` object exists, so a new sample is visible from every path.
  leaf.history.push({ time: '', delay: 75 })
  expect(memberDelay('auto', root, data, D)).toBe(75)
})

test('resolves a hidden group that is not listed at the top level', () => {
  const leaf = node('leaf', [42])
  const hidden = groupRecord('hidden', 'leaf', ['leaf'])
  const root = group('root', 'hidden', ['hidden'])
  const data = snapshot(root, [root], { hidden, leaf })
  expect(memberDelay('hidden', root, data, D)).toBe(42)
})

test('missing selections, unresolved nodes, and cycles have no delay', () => {
  const root = group('root', 'missing', [])
  const data = snapshot(root, [root], {})
  expect(memberDelay('missing', root, data, D)).toBe(undefined)
  data.nodes.root = groupRecord('root', 'nested', ['nested'])
  data.nodes.nested = groupRecord('nested', 'root', ['root'])
  expect(memberDelay('nested', root, data, D)).toBe(undefined)
  expect(memberDelay('root', root, data, D)).toBe(undefined)
})
