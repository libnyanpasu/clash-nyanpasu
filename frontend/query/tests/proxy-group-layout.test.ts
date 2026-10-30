import { expect, test } from 'vitest'
import type { ProxyGroupFolder, ProxyGroupLayout } from '@nyanpasu/rpc/types'
import {
  applyProxyGroupLayout,
  flatProxyGroupOrder,
  resolveProxyGroupLayout,
  storedProxyGroupLayoutOf,
} from '../src/ipc/proxy-group-layout'

const folder = (
  id: string,
  name: string,
  members: string[],
): ProxyGroupFolder => ({ id, name, members })

const layout = (
  ungrouped: string[],
  folders: ProxyGroupFolder[] = [],
): ProxyGroupLayout => ({ folders, ungrouped })

// These cases mirror the Rust `ProxyGroupLayout::resolve` tests in
// `nyanpasu-config`: the two implementations are one contract.

test('an empty layout resolves to the core order', () => {
  expect(resolveProxyGroupLayout(undefined, ['B', 'A', 'C'])).toEqual(
    layout(['B', 'A', 'C']),
  )
  expect(resolveProxyGroupLayout(layout([]), ['B', 'A', 'C'])).toEqual(
    layout(['B', 'A', 'C']),
  )
})

test('resolve keeps the stored order and folders', () => {
  const stored = layout(['C', 'A'], [folder('f1', 'Streaming', ['E', 'D'])])

  expect(resolveProxyGroupLayout(stored, ['A', 'C', 'D', 'E'])).toEqual(stored)
})

test('resolve ignores groups that are not live', () => {
  const stored = layout(['Gone', 'A'], [folder('f1', 'F', ['Gone', 'D'])])

  expect(resolveProxyGroupLayout(stored, ['A', 'D'])).toEqual(
    layout(['A'], [folder('f1', 'F', ['D'])]),
  )
})

test('resolve keeps a folder whose members are all gone', () => {
  const stored = layout(['A'], [folder('f1', 'F', ['Gone'])])

  expect(resolveProxyGroupLayout(stored, ['A'])).toEqual(
    layout(['A'], [folder('f1', 'F', [])]),
  )
})

test('resolve appends new groups after the placed ungrouped ones in core order', () => {
  const stored = layout(['B'], [folder('f1', 'F', ['D'])])

  expect(resolveProxyGroupLayout(stored, ['X', 'B', 'D', 'Y'])).toEqual(
    layout(['B', 'X', 'Y'], [folder('f1', 'F', ['D'])]),
  )
})

test('resolve keeps the first place of a name placed twice', () => {
  const stored = layout(['A', 'A'], [folder('f1', 'F', ['A', 'B'])])

  expect(resolveProxyGroupLayout(stored, ['A', 'B'])).toEqual(
    layout(['A'], [folder('f1', 'F', ['B'])]),
  )
})

test('resolve ignores duplicate live names', () => {
  expect(resolveProxyGroupLayout(undefined, ['A', 'A', 'B'])).toEqual(
    layout(['A', 'B']),
  )
})

test('resolve does not change the stored layout', () => {
  const stored = layout(['Gone', 'A'], [folder('f1', 'F', ['D'])])
  const snapshot = structuredClone(stored)

  resolveProxyGroupLayout(stored, ['A'])

  expect(stored).toEqual(snapshot)
})

test('the flat order is the sidebar from top to bottom', () => {
  const stored = layout(
    ['C', 'A'],
    [folder('f1', 'One', ['E', 'D']), folder('f2', 'Two', ['F'])],
  )

  expect(
    flatProxyGroupOrder(
      resolveProxyGroupLayout(stored, ['A', 'C', 'D', 'E', 'F', 'G']),
    ),
  ).toEqual(['C', 'A', 'G', 'E', 'D', 'F'])
})

test('the stored layout is the one of the active profile', () => {
  const source = {
    current: 'b',
    proxy_group_layouts: { a: layout(['X']), b: layout(['Y']) },
  }

  expect(storedProxyGroupLayoutOf(source)).toEqual(layout(['Y']))
  expect(storedProxyGroupLayoutOf({ ...source, current: 'c' })).toBeUndefined()
  expect(storedProxyGroupLayoutOf({ ...source, current: null })).toBeUndefined()
  expect(storedProxyGroupLayoutOf(undefined)).toBeUndefined()
})

test('applying a layout replaces the entry of the profile only', () => {
  const source = {
    current: 'a',
    proxy_group_layouts: { a: layout(['X']), b: layout(['Y']) },
  }

  expect(
    applyProxyGroupLayout(source, 'a', layout(['Z'])).proxy_group_layouts,
  ).toEqual({ a: layout(['Z']), b: layout(['Y']) })
  expect(source.proxy_group_layouts.a).toEqual(layout(['X']))
})

test('applying an empty layout removes the entry, like the backend', () => {
  const source = {
    current: 'a',
    proxy_group_layouts: { a: layout(['X']), b: layout(['Y']) },
  }

  expect(
    applyProxyGroupLayout(source, 'a', layout([])).proxy_group_layouts,
  ).toEqual({ b: layout(['Y']) })
  expect(
    applyProxyGroupLayout({ current: 'a' }, 'a', layout(['X']))
      .proxy_group_layouts,
  ).toEqual({ a: layout(['X']) })
})
