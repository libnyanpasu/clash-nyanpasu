import type { LaneLayout } from './inspect-lanes'
import type { Stage, Step } from './inspect-steps'

// Pure geometry of the graph view, in unscaled pixels.

export const NODE_WIDTH = 220
export const NODE_HEIGHT = 44

const LANE_PITCH = 256
const ROW_PITCH = 76
const PADDING = 16
const BAND_PADDING_X = 24
// Room above the first node of a band for its label.
const BAND_TOP = 48
const BAND_BOTTOM = 20
const BAND_GAP = 12
const CORNER = 8

export type GraphNode = { key: string; x: number; y: number }

export type GraphBand = {
  stage: Stage
  x: number
  y: number
  width: number
  height: number
}

export type GraphPath = {
  from: string
  to: string
  /** The SVG path of the edge, from the bottom of one node to the top of the next. */
  d: string
}

export type GraphGeometry = {
  nodes: GraphNode[]
  bands: GraphBand[]
  edges: GraphPath[]
  width: number
  height: number
}

/**
 * A path down from (x1, y1) to (x2, y2) that runs in the lane at `lane` between
 * the two, with rounded elbows in the gap under the first node and above the
 * second.
 */
export function edgePath(
  start: { x: number; y: number },
  end: { x: number; y: number },
  lane: number,
): string {
  const turn = (from: number, to: number, y: number) => {
    const dir = Math.sign(to - from)
    return [
      `Q ${from} ${y + CORNER} ${from + dir * CORNER} ${y + CORNER}`,
      `H ${to - dir * CORNER}`,
      `Q ${to} ${y + CORNER} ${to} ${y + CORNER * 2}`,
    ].join(' ')
  }

  const parts = [`M ${start.x} ${start.y}`]
  const out = start.y + 4
  const into = end.y - CORNER * 2 - 4

  if (lane !== start.x) {
    parts.push(`V ${out}`, turn(start.x, lane, out))
  }
  parts.push(`V ${into}`)
  if (lane !== end.x) {
    parts.push(turn(lane, end.x, into))
  }
  parts.push(`V ${end.y}`)

  return parts.join(' ')
}

/** Places the visible steps by lane and row, and the stages as bands. */
export function layoutGraph(steps: Step[], lanes: LaneLayout): GraphGeometry {
  const laneX = (lane: number) =>
    PADDING + BAND_PADDING_X + lane * LANE_PITCH + NODE_WIDTH / 2
  const rowOf = new Map(lanes.rows.map((row) => [row.key, row]))

  const nodes: GraphNode[] = []
  const bands: GraphBand[] = []
  const width =
    PADDING * 2 +
    BAND_PADDING_X * 2 +
    (lanes.laneCount - 1) * LANE_PITCH +
    NODE_WIDTH

  let y = PADDING
  let band: GraphBand | undefined
  let rowTop = 0

  for (const step of steps) {
    if (step.stage !== band?.stage) {
      if (band) y += BAND_BOTTOM + BAND_GAP
      band = {
        stage: step.stage,
        x: PADDING,
        y,
        width: width - PADDING * 2,
        height: 0,
      }
      bands.push(band)
      rowTop = y + BAND_TOP
    } else {
      rowTop += ROW_PITCH
    }

    nodes.push({
      key: step.key,
      x: laneX(rowOf.get(step.key)!.lane) - NODE_WIDTH / 2,
      y: rowTop,
    })
    band.height = rowTop + NODE_HEIGHT + BAND_BOTTOM - band.y
    y = band.y + band.height
  }

  const at = new Map(nodes.map((node) => [node.key, node]))
  const edges = lanes.edges.map(({ from, to, lane }): GraphPath => {
    const source = at.get(from)!
    const target = at.get(to)!
    return {
      from,
      to,
      d: edgePath(
        { x: source.x + NODE_WIDTH / 2, y: source.y + NODE_HEIGHT },
        { x: target.x + NODE_WIDTH / 2, y: target.y },
        laneX(lane),
      ),
    }
  })

  return { nodes, bands, edges, width, height: y + PADDING }
}

/** The selected step with every step that leads to it and every one it leads to. */
export function traceOf(
  edges: readonly { from: string; to: string }[],
  selectedKey: string,
): Set<string> {
  const trace = new Set([selectedKey])
  const walk = (near: 'from' | 'to', far: 'from' | 'to') => {
    const queue = [selectedKey]
    for (let key = queue.pop(); key !== undefined; key = queue.pop()) {
      for (const edge of edges) {
        if (edge[near] === key && !trace.has(edge[far])) {
          trace.add(edge[far])
          queue.push(edge[far])
        }
      }
    }
  }
  walk('to', 'from')
  walk('from', 'to')
  return trace
}
