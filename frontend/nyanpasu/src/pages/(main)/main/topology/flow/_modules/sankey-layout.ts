import { usageValue } from '@/utils/traffic-usage'
import type { Metric, TopologyEdge, TopologyNode } from '@nyanpasu/rpc/types'

export const NODE_WIDTH = 8
export const NODE_GAP = 10
/** The height a row of the tallest column needs, label included. */
export const ROW_HEIGHT = 22
const MIN_NODE_HEIGHT = 2

export type SankeyNode = {
  node: TopologyNode
  x: number
  y: number
  height: number
}

export type SankeyBand = {
  id: string
  source: string
  target: string
  /** The filled outline of the band. */
  path: string
  /** Where the middle of the band runs, drawn as wide as the band is thick. */
  centerline: string
  thickness: number
}

export const edgeId = ({ source, target }: TopologyEdge) =>
  `${source}->${target}`

/** The height under which the tallest column cannot show all of its nodes. */
export function sankeyMinHeight(nodes: readonly TopologyNode[]) {
  const rows = new Map<number, number>()

  for (const { layer } of nodes) rows.set(layer, (rows.get(layer) ?? 0) + 1)

  return Math.max(0, ...rows.values()) * ROW_HEIGHT
}

/**
 * Places the nodes in columns, the heaviest first as the backend sorts them,
 * and joins them with bands. One scale serves every column, so a band is as
 * thick where it leaves a node as where it arrives.
 */
export function layoutSankey({
  nodes,
  edges,
  layerCount,
  metric,
  width,
  height,
}: {
  nodes: readonly TopologyNode[]
  edges: readonly TopologyEdge[]
  layerCount: number
  metric: Metric
  width: number
  height: number
}) {
  const columns = Array.from({ length: layerCount }, () => [] as TopologyNode[])

  for (const node of nodes) columns[node.layer]?.push(node)

  let scale = Infinity

  for (const column of columns) {
    const total = column.reduce(
      (sum, { usage }) => sum + usageValue(usage, metric),
      0,
    )

    if (total > 0) {
      scale = Math.min(
        scale,
        Math.max(0, height - NODE_GAP * (column.length - 1)) / total,
      )
    }
  }

  if (!Number.isFinite(scale)) scale = 0

  const step = layerCount > 1 ? (width - NODE_WIDTH) / (layerCount - 1) : 0
  const placed = new Map<string, SankeyNode>()

  columns.forEach((column, layer) => {
    let y = 0

    for (const node of column) {
      const nodeHeight = Math.max(
        usageValue(node.usage, metric) * scale,
        MIN_NODE_HEIGHT,
      )

      placed.set(node.id, { node, x: layer * step, y, height: nodeHeight })
      y += nodeHeight + NODE_GAP
    }
  })

  const drawn = edges.filter(
    (edge) =>
      placed.has(edge.source) &&
      placed.has(edge.target) &&
      usageValue(edge.usage, metric) > 0,
  )

  // Within a node the bands are stacked in the order of the node they meet,
  // which keeps them from crossing each other next to the node.
  const stack = (end: 'source' | 'target', other: 'source' | 'target') => {
    const groups = Map.groupBy(drawn, (edge) => edge[end])
    const offsets = new Map<TopologyEdge, number>()

    for (const [id, group] of groups) {
      let y = placed.get(id)!.y

      for (const edge of group.toSorted(
        (a, b) => placed.get(a[other])!.y - placed.get(b[other])!.y,
      )) {
        offsets.set(edge, y)
        y += usageValue(edge.usage, metric) * scale
      }
    }

    return offsets
  }

  const leaving = stack('source', 'target')
  const arriving = stack('target', 'source')

  const bands = drawn.map<SankeyBand>((edge) => {
    const thickness = usageValue(edge.usage, metric) * scale
    const from = placed.get(edge.source)!
    const to = placed.get(edge.target)!
    const x0 = from.x + NODE_WIDTH
    const x1 = to.x
    const xm = (x0 + x1) / 2
    const y0 = leaving.get(edge)!
    const y1 = arriving.get(edge)!

    return {
      id: edgeId(edge),
      source: edge.source,
      target: edge.target,
      path: `M${x0} ${y0}C${xm} ${y0} ${xm} ${y1} ${x1} ${y1}V${y1 + thickness}C${xm} ${y1 + thickness} ${xm} ${y0 + thickness} ${x0} ${y0 + thickness}Z`,
      centerline: `M${x0} ${y0 + thickness / 2}C${xm} ${y0 + thickness / 2} ${xm} ${y1 + thickness / 2} ${x1} ${y1 + thickness / 2}`,
      thickness,
    }
  })

  return { nodes: [...placed.values()], bands, step }
}
