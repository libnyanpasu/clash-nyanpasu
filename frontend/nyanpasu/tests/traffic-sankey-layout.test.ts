import { expect, test } from 'vitest'
import type { TopologyEdge, TopologyNode } from '@nyanpasu/rpc/types'
import {
  layoutSankey,
  NODE_GAP,
  sankeyMinHeight,
} from '../src/pages/(main)/main/topology/flow/_modules/sankey-layout'

const usage = (download: number) => ({
  bytes: { upload: 0, download },
  connections: 1,
})
const node = (id: string, layer: number, download: number): TopologyNode => ({
  id,
  layer,
  key: id,
  usage: usage(download),
})
const edge = (
  source: string,
  target: string,
  download: number,
): TopologyEdge => ({
  source,
  target,
  usage: usage(download),
})

const nodes = [
  node('a', 0, 60),
  node('b', 0, 40),
  node('x', 1, 70),
  node('y', 1, 30),
]
const edges = [
  edge('a', 'x', 50),
  edge('a', 'y', 10),
  edge('b', 'x', 20),
  edge('b', 'y', 20),
]

const layout = () =>
  layoutSankey({
    nodes,
    edges,
    layerCount: 2,
    metric: 'bytes',
    width: 408,
    height: 210,
  })

test('the nodes of a column are stacked by value and the columns span the width', () => {
  const placed = new Map(layout().nodes.map((item) => [item.node.id, item]))

  expect(placed.get('a')).toMatchObject({ x: 0, y: 0 })
  expect(placed.get('x')).toMatchObject({ x: 400, y: 0 })
  // One scale for every column: 100 bytes take the height without the gaps.
  expect(placed.get('a')!.height).toBeCloseTo(0.6 * (210 - NODE_GAP))
  expect(placed.get('b')!.y).toBeCloseTo(placed.get('a')!.height + NODE_GAP)
})

test('a band is as thick as its value on the shared scale', () => {
  const bands = layout().bands

  expect(bands).toHaveLength(4)
  expect(bands.find((band) => band.id === 'a->x')!.thickness).toBeCloseTo(
    0.5 * (210 - NODE_GAP),
  )
})

test('a band without value or without both nodes is not drawn', () => {
  const result = layoutSankey({
    nodes,
    edges: [...edges, edge('a', 'missing', 5), edge('b', 'x', 0)],
    layerCount: 2,
    metric: 'bytes',
    width: 408,
    height: 210,
  })

  expect(result.bands.map((band) => band.id)).not.toContain('a->missing')
  expect(result.bands).toHaveLength(4)
})

test('a report without traffic has nodes of the minimum height', () => {
  const result = layoutSankey({
    nodes: [node('a', 0, 0), node('x', 1, 0)],
    edges: [],
    layerCount: 2,
    metric: 'bytes',
    width: 100,
    height: 100,
  })

  expect(result.nodes.every((item) => item.height > 0)).toBe(true)
})

test('the minimum height follows the tallest column', () => {
  expect(sankeyMinHeight([])).toBe(0)
  expect(sankeyMinHeight(nodes)).toBe(2 * sankeyMinHeight([nodes[0]]))
})
