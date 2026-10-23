import type { Step } from './inspect-steps'

// Pure lane layout of the build graph. The list draws it as a gutter and the
// graph as nodes and edges, so both read the same rows.

export type LaneShape = 'start' | 'merge' | 'fork' | 'step'

export type GraphEdge = {
  from: string
  to: string
  /** The lane the edge runs in between its two steps. */
  lane: number
}

export type LaneRow = {
  key: string
  /** The lane of the step itself. */
  lane: number
  shape: LaneShape
  /** Whether a line arrives from above and leaves below the step. */
  incoming: boolean
  outgoing: boolean
  /** Lanes that end at this step, besides its own. */
  merges: number[]
  /** Lanes that start at this step, besides its own. */
  forks: number[]
  /** Lanes that pass the row without touching the step. */
  through: number[]
  /** Lanes that are drawn below the row, up to the next one. */
  after: number[]
}

export type LaneLayout = {
  rows: LaneRow[]
  edges: GraphEdge[]
  laneCount: number
}

/**
 * The edges between the given steps, as step keys. A step that is not in the
 * list is bypassed, so its neighbours stay joined. Edges that do not point
 * forward in the list are left out, because they cannot be drawn downward.
 */
export function stepEdges(
  all: Step[],
  visible: Step[],
): { from: string; to: string }[] {
  const byId = new Map(all.map((step) => [step.node.id, step]))
  const order = new Map(visible.map((step, index) => [step.key, index]))
  const edges = new Map<string, { from: string; to: string }>()

  const reach = (id: number, seen: Set<number>): string[] => {
    const step = byId.get(id)
    if (!step || seen.has(id)) return []
    seen.add(id)
    return order.has(step.key)
      ? [step.key]
      : step.node.next.flatMap((next) => reach(next, seen))
  }

  for (const step of visible) {
    for (const next of step.node.next) {
      for (const to of reach(next, new Set())) {
        if (order.get(to)! > order.get(step.key)!)
          edges.set(`${step.key}\n${to}`, { from: step.key, to })
      }
    }
  }
  return [...edges.values()]
}

/**
 * Assigns each visible step a lane. The first successor of a step continues
 * in its lane, every further one opens a lane, and a step that several lanes
 * lead into closes them.
 */
export function layoutLanes(all: Step[], visible: Step[]): LaneLayout {
  const order = new Map(visible.map((step, index) => [step.key, index]))
  const successors = new Map<string, string[]>()
  for (const { from, to } of stepEdges(all, visible)) {
    successors.set(from, [...(successors.get(from) ?? []), to])
  }

  // The key each lane is heading for; null for a free lane.
  const lanes: (string | null)[] = []
  const firstFree = () => {
    const free = lanes.indexOf(null)
    return free === -1 ? lanes.length : free
  }
  const active = () =>
    lanes.flatMap((target, lane) => (target === null ? [] : [lane]))

  const rows: LaneRow[] = []
  const edges: GraphEdge[] = []

  for (const step of visible) {
    const arriving = active().filter((lane) => lanes[lane] === step.key)
    const lane = arriving[0] ?? firstFree()
    const before = active()

    for (const arrived of arriving) lanes[arrived] = null
    lanes[lane] = null

    const next = (successors.get(step.key) ?? []).sort(
      (a, b) => order.get(a)! - order.get(b)!,
    )
    const forks: number[] = []
    next.forEach((to, index) => {
      const target = index === 0 ? lane : firstFree()
      lanes[target] = to
      if (index > 0) forks.push(target)
      edges.push({ from: step.key, to, lane: target })
    })

    const merges = arriving.slice(1)
    rows.push({
      key: step.key,
      lane,
      shape:
        arriving.length === 0
          ? 'start'
          : merges.length > 0
            ? 'merge'
            : forks.length > 0
              ? 'fork'
              : 'step',
      incoming: arriving.length > 0,
      outgoing: next.length > 0,
      merges,
      forks,
      through: before.filter((other) => !arriving.includes(other)),
      after: active(),
    })
  }

  return {
    rows,
    edges,
    laneCount: Math.max(
      1,
      ...rows
        .flatMap((row) => [row.lane, ...row.forks, ...row.after])
        .map((lane) => lane + 1),
    ),
  }
}
