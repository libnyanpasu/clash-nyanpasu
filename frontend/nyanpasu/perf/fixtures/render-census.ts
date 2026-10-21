// Counts the components each commit renders, by name, through the hook React
// DevTools use. It is installed before React loads, and only with
// `VITE_PERF_RENDERS`: React reports every commit to an installed hook.

type Fiber = {
  tag: number
  type: unknown
  flags: number
  actualStartTime?: number
  alternate: Fiber | null
  child: Fiber | null
  sibling: Fiber | null
  return: Fiber | null
}

/** Mounts and updates of a component, by its name. */
export type RenderCounts = Record<string, [mounts: number, updates: number]>

/** What the commits of one measured action rendered. */
export type Renders = {
  commits: number
  /** Every component each commit rendered. */
  all: RenderCounts
  /** The commit that rendered the most components. */
  largest: RenderCounts
  /**
   * Components that rendered again while their parent did not, by the names
   * of the component and its nearest parents: a state, store or context
   * update started there.
   */
  unprompted: Record<string, number>
}

// Function, class, forward ref, memo and simple memo components.
const COMPONENT_TAGS = new Set([0, 1, 11, 14, 15])
// Set on a fiber whose component function ran and rendered.
const PERFORMED_WORK = 1

const nameOf = (type: unknown): string => {
  const value = type as {
    displayName?: string
    name?: string
    render?: unknown
    type?: unknown
  } | null
  return (
    value?.displayName ||
    value?.name ||
    (value?.render ? nameOf(value.render) : undefined) ||
    (value?.type ? nameOf(value.type) : '(anonymous)')
  )
}

// The end of the previous commit: a fiber started after it rendered in this
// commit. A fiber left out of a commit keeps the flags of its last render.
let previousCommit = 0

const add = (counts: RenderCounts, name: string, mounted: boolean) => {
  const count = (counts[name] ??= [0, 0])
  count[mounted ? 0 : 1] += 1
}

const rendered = (fiber: Fiber) =>
  COMPONENT_TAGS.has(fiber.tag) &&
  (fiber.actualStartTime ?? -1) > previousCommit &&
  (fiber.alternate === null || (fiber.flags & PERFORMED_WORK) !== 0)

const parentOf = (fiber: Fiber) => {
  let parent = fiber.return
  while (parent && !COMPONENT_TAGS.has(parent.tag)) parent = parent.return
  return parent
}

function countCommit(root: Fiber, renders: Renders) {
  const commit: RenderCounts = {}
  const stack: (Fiber | null)[] = [root]

  while (stack.length) {
    const fiber = stack.pop()
    if (!fiber) continue
    stack.push(fiber.sibling, fiber.child)
    if (!rendered(fiber)) continue

    const name = nameOf(fiber.type)
    const mounted = fiber.alternate === null
    add(commit, name, mounted)
    add(renders.all, name, mounted)

    const parent = parentOf(fiber)
    if (!mounted && parent && !rendered(parent)) {
      const chain = [name]
      for (let up: Fiber | null = parent; up && chain.length < 4;) {
        chain.push(nameOf(up.type))
        up = parentOf(up)
      }
      const key = chain.join(' < ')
      renders.unprompted[key] = (renders.unprompted[key] ?? 0) + 1
    }
  }

  const size = (counts: RenderCounts) =>
    Object.values(counts).reduce((sum, [m, u]) => sum + m + u, 0)
  if (size(commit) > size(renders.largest)) renders.largest = commit
}

type Census = { counting: Renders | null }

// The setup file and the benchmark each load this module, so the hook React
// reports to holds the state both see.
const census = (): Census | undefined =>
  (window as { __REACT_DEVTOOLS_GLOBAL_HOOK__?: { census?: Census } })
    .__REACT_DEVTOOLS_GLOBAL_HOOK__?.census

if (import.meta.env.VITE_PERF_RENDERS && !census()) {
  const state: Census = { counting: null }

  Object.assign(window, {
    __REACT_DEVTOOLS_GLOBAL_HOOK__: {
      census: state,
      supportsFiber: true,
      renderers: new Map(),
      inject: () => 1,
      checkDCE: () => {},
      onScheduleFiberRoot: () => {},
      onCommitFiberRoot: (_id: number, root: { current: Fiber }) => {
        if (state.counting) {
          state.counting.commits += 1
          countCommit(root.current, state.counting)
        }
        previousCommit = performance.now()
      },
      onCommitFiberUnmount: () => {},
      onPostCommitFiberRoot: () => {},
    },
  })
}

/** Starts counting renders, if `VITE_PERF_RENDERS` is set. */
export function startCountingRenders() {
  const state = census()
  if (state) {
    state.counting = { commits: 0, all: {}, largest: {}, unprompted: {} }
  }
}

/** Stops counting, returning what was rendered since it started. */
export function stopCountingRenders(): Renders | undefined {
  const state = census()
  const renders = state?.counting ?? undefined
  if (state) state.counting = null
  return renders
}

/**
 * The renders of several actions, each count averaged per action and the
 * most frequent first, cut to `top` entries each.
 */
export function summarizeRenders(samples: Renders[], top = 40) {
  const per = (value: number) => Number((value / samples.length).toFixed(1))
  const sum = (pick: (renders: Renders) => RenderCounts) => {
    const total: RenderCounts = {}
    for (const renders of samples) {
      for (const [name, [m, u]] of Object.entries(pick(renders))) {
        const count = (total[name] ??= [0, 0])
        count[0] += m
        count[1] += u
      }
    }
    return Object.fromEntries(
      Object.entries(total)
        .sort(([, a], [, b]) => b[0] + b[1] - a[0] - a[1])
        .slice(0, top)
        .map(([name, [m, u]]) => [name, [per(m), per(u)]]),
    )
  }
  const unprompted: Record<string, number> = {}
  for (const renders of samples) {
    for (const [key, count] of Object.entries(renders.unprompted)) {
      unprompted[key] = (unprompted[key] ?? 0) + count
    }
  }

  return {
    commits: per(samples.reduce((sum, { commits }) => sum + commits, 0)),
    all: sum((renders) => renders.all),
    largest: sum((renders) => renders.largest),
    unprompted: Object.fromEntries(
      Object.entries(unprompted)
        .sort(([, a], [, b]) => b - a)
        .slice(0, top)
        .map(([key, count]) => [key, per(count)]),
    ),
  }
}
