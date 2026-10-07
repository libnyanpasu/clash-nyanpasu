import { expect, test, vi } from 'vitest'
import type {
  Proxies_Serialize,
  Proxy_Serialize,
  ProxyGroup,
} from '@nyanpasu/rpc/types'
import {
  DEFAULT_NODE_VIEW,
  matchesNodeSearch,
  toNodeView,
  visibleMembers,
  type NodeView,
} from '../src/pages/(main)/main/proxies/group/_modules/node-list'

const node = (name: string): Proxy_Serialize => ({
  name,
  type: 'Direct',
  udp: false,
  history: [],
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
const group = (now: string | null, all: string[]): ProxyGroup => ({
  name: 'G',
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
const snapshot = (names: string[]): Proxies_Serialize => ({
  global: null,
  groups: [],
  nodes: Object.fromEntries(names.map((name) => [name, node(name)])),
})
const run = (
  members: string[],
  delays: Record<string, number | undefined>,
  view: Partial<NodeView>,
  options: { now?: string | null; query?: string; known?: string[] } = {},
) =>
  visibleMembers({
    group: group(options.now ?? null, members),
    proxies: snapshot(options.known ?? members),
    query: options.query ?? '',
    view: { ...DEFAULT_NODE_VIEW, ...view },
    delayOf: (name) => delays[name],
  })

test('every space-separated term must match the name or type, ignoring case', () => {
  const n = { ...node('HK 香港-01'), type: 'Vless' }
  expect(matchesNodeSearch(n, '')).toBe(true)
  expect(matchesNodeSearch(n, '  ')).toBe(true)
  expect(matchesNodeSearch(n, 'hk')).toBe(true)
  expect(matchesNodeSearch(n, '香港 vless')).toBe(true)
  expect(matchesNodeSearch(n, ' HK   01 ')).toBe(true)
  expect(matchesNodeSearch(n, 'hk jp')).toBe(false)
})

test('names sort naturally and delays ascend with untested then failed last', () => {
  const delays = { a: 50, b: undefined, c: 0, d: 10 }
  const members = ['a', 'b', 'c', 'd', 'HK-10', 'HK-2']

  expect(run(members, delays, { sort: 'name' })).toEqual([
    'a',
    'b',
    'c',
    'd',
    'HK-2',
    'HK-10',
  ])
  expect(run(['a', 'b', 'c', 'd'], delays, { sort: 'delay' })).toEqual([
    'd',
    'a',
    'b',
    'c',
  ])
  expect(run(members, delays, { sort: 'default' })).toEqual(members)
})

test('hiding unavailable members keeps untested ones and the current selection', () => {
  const delays = { ok: 20, failed: 0, untested: undefined, current: 0 }

  expect(
    run(
      ['ok', 'failed', 'untested', 'current'],
      delays,
      { hideUnavailable: true },
      { now: 'current' },
    ),
  ).toEqual(['ok', 'untested', 'current'])
})

test('search and filters combine; unknown members are dropped', () => {
  const delays = { 'hk-1': 30, 'hk-2': 0, 'hk-3': 10, 'jp-1': 5, ghost: 1 }

  expect(
    run(
      ['hk-1', 'hk-2', 'ghost', 'hk-3', 'jp-1'],
      delays,
      { sort: 'delay', hideUnavailable: true },
      { query: 'HK', known: ['hk-1', 'hk-2', 'hk-3', 'jp-1'] },
    ),
  ).toEqual(['hk-3', 'hk-1'])
})

test('delays are resolved only when sorting or filtering needs them, once per member', () => {
  const members = ['a', 'b', 'c']
  const resolve = (view: Partial<NodeView>) => {
    const delayOf = vi.fn((name: string) => (name === 'a' ? 10 : undefined))
    visibleMembers({
      group: group(null, members),
      proxies: snapshot(members),
      query: '',
      view: { ...DEFAULT_NODE_VIEW, ...view },
      delayOf,
    })
    return delayOf
  }

  expect(resolve({ sort: 'default', hideUnavailable: false })).not.toBeCalled()
  expect(resolve({ sort: 'name', hideUnavailable: false })).not.toBeCalled()
  expect(resolve({ sort: 'delay' }).mock.calls).toEqual([['a'], ['b'], ['c']])
  expect(resolve({ hideUnavailable: true }).mock.calls).toEqual([
    ['a'],
    ['b'],
    ['c'],
  ])
  expect(
    resolve({ sort: 'delay', hideUnavailable: true }).mock.calls,
  ).toHaveLength(3)
})

test('a stored view is coerced', () => {
  expect(toNodeView(null)).toEqual(DEFAULT_NODE_VIEW)
  expect(toNodeView({ sort: 'delay', hideUnavailable: true })).toEqual({
    sort: 'delay',
    hideUnavailable: true,
  })
  expect(toNodeView({ sort: 'bogus', hideUnavailable: 'yes' })).toEqual(
    DEFAULT_NODE_VIEW,
  )
  expect(toNodeView({ sort: 'name', hideUnavailable: 'yes' })).toEqual({
    sort: 'name',
    hideUnavailable: false,
  })
})
