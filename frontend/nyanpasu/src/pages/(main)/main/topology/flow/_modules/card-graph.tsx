import { AnimatePresence, motion } from 'motion/react'
import { useMemo, useState, type ReactNode } from 'react'
import { m } from '@/paraglide/messages'
import {
  dimensionName,
  largest,
  usageAmount,
  usageValue,
} from '@/utils/traffic-usage'
import type { Dimension, Topology, TopologyNode } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useTrafficPage } from '../../_modules/traffic-context'
import { usePageTransition } from '../../_modules/transition'
import { toggleFilter } from '../../../_modules/traffic-filters'

const tones = [
  'bg-primary-container text-on-primary-container',
  'bg-secondary-container text-on-secondary-container',
  'bg-tertiary-container text-on-tertiary-container',
  'bg-primary-container text-on-primary-container',
]
const colors = ['primary', 'secondary', 'tertiary', 'primary']
const NODE_WIDTH = 190
const COLUMN_STEP = 270
const ROW_STEP = 76
// Above this many nodes or edges the drawing is static: animating and
// re-rendering each of them on every hover would stutter.
const LARGE_GRAPH = 200
// A taller column than this packs every column from the top; centering the
// short ones against it would push them far down the page.
const CENTERED_ROWS = 16

/** Exit animations only matter while the drawing is animated at all. */
function Presence({
  large,
  children,
}: {
  large: boolean
  children: ReactNode
}) {
  return large ? (
    children
  ) : (
    <AnimatePresence initial={false}>{children}</AnimatePresence>
  )
}

/**
 * The layers as columns of cards joined by curves as thick as their value:
 * the flow page's drawing before the Sankey diagram, kept as an alternative.
 */
export default function CardGraph({
  layers,
  topology,
}: {
  layers: readonly Dimension[]
  topology: Topology
}) {
  const { search, labelOf, onSearchChange } = useTrafficPage()
  const { metric, filters } = search
  const { transition } = usePageTransition()
  const [hovered, setHovered] = useState<string>()
  const graphWidth = COLUMN_STEP * (layers.length - 1) + NODE_WIDTH

  const columns = useMemo(() => {
    const result: TopologyNode[][] = layers.map(() => [])
    for (const node of topology.nodes) result[node.layer]?.push(node)
    return result
  }, [topology, layers])
  const nodes = columns.flat()
  const nodeById = new Map(nodes.map((node) => [node.id, node]))
  const edges = topology.edges.filter(
    (edge) => nodeById.has(edge.source) && nodeById.has(edge.target),
  )
  const highlighting = hovered !== undefined && nodeById.has(hovered)
  const large = nodes.length > LARGE_GRAPH || edges.length > LARGE_GRAPH

  const rows = largest(
    columns.map((column) => column.length),
    3,
  )
  const height = rows * ROW_STEP
  const centered = !large && rows <= CENTERED_ROWS
  const positions = new Map(
    columns.flatMap((column, index) =>
      column.map(
        (node, row) =>
          [
            node.id,
            {
              x: index * COLUMN_STEP,
              y:
                row * ROW_STEP +
                (centered ? (height - column.length * ROW_STEP) / 2 : 0),
            },
          ] as const,
      ),
    ),
  )
  const maximum = largest(
    edges.map((edge) => usageValue(edge.usage, metric)),
    1,
  )
  const viewBox = `0 0 ${graphWidth} ${height}`

  return (
    <div
      // The padding leaves room for the focus ring (2px + 2px offset) of the
      // nodes at the edges, which the scroll container would clip.
      // The drawing scrolls inside the card instead of stretching it.
      className="min-h-0 flex-1 overflow-auto p-2"
      tabIndex={0}
      role="region"
      aria-label={m.topology_title()}
      data-slot="flow-card-graph"
    >
      <div style={{ minWidth: graphWidth }}>
        <div className="bg-surface sticky top-0 z-20 flex justify-between pb-3 text-xs font-medium tracking-wide">
          {layers.map((dimension, index) => (
            <div
              key={dimension}
              style={{ width: `${(NODE_WIDTH / graphWidth) * 100}%` }}
              className="text-on-surface-variant flex gap-2 px-3"
            >
              <span className="text-outline">0{index + 1}</span>
              {dimensionName(dimension)}
            </div>
          ))}
        </div>
        <motion.div
          className="relative"
          {...(large
            ? { style: { height } }
            : { initial: false, animate: { height }, transition })}
        >
          <motion.svg
            {...(large
              ? { viewBox }
              : {
                  initial: false,
                  animate: { viewBox },
                  transition,
                })}
            preserveAspectRatio="none"
            className="pointer-events-none absolute inset-0 size-full"
            aria-hidden="true"
          >
            <Presence large={large}>
              {edges.map((edge) => {
                const start = positions.get(edge.source)!
                const end = positions.get(edge.target)!
                const x = start.x + NODE_WIDTH
                const y = start.y + 28
                const value = usageValue(edge.usage, metric)
                if (!value) return null
                const d = `M ${x} ${y} C ${x + 40} ${y}, ${end.x - 40} ${end.y + 28}, ${end.x} ${end.y + 28}`
                const strokeWidth = 1 + (value / maximum) * 13
                const opacity = !highlighting
                  ? 0.25
                  : edge.source === hovered || edge.target === hovered
                    ? 0.65
                    : 0.04
                return (
                  <motion.path
                    key={`${edge.source}->${edge.target}`}
                    fill="none"
                    stroke={`var(--color-md-${colors[Math.round(start.x / COLUMN_STEP) % colors.length]})`}
                    {...(large
                      ? { d, strokeWidth, opacity }
                      : {
                          d,
                          initial: { opacity: 0 },
                          animate: { d, strokeWidth, opacity },
                          exit: { opacity: 0 },
                          transition,
                        })}
                  />
                )
              })}
            </Presence>
          </motion.svg>
          <Presence large={large}>
            {nodes.map((node) => {
              const position = positions.get(node.id)!
              const dimension = layers[node.layer]
              const { key } = node
              const merged = key === null
              // The node that merges the rest of the layer has no value.
              const info = key === null ? null : labelOf(dimension, key)
              const text = info?.text ?? m.topology_other()
              const amount = usageAmount(node.usage, metric)
              const active =
                !merged &&
                filters.some(
                  (filter) => filter.d === dimension && filter.v === key,
                )
              const Node = merged ? motion.div : motion.button
              return (
                <Node
                  key={node.id}
                  {...(merged
                    ? {}
                    : {
                        type: 'button' as const,
                        'aria-pressed': active,
                        onClick: () =>
                          onSearchChange({
                            filters: toggleFilter(filters, dimension, key),
                          }),
                      })}
                  aria-label={`${dimensionName(dimension)}: ${text}, ${amount}`}
                  title={`${info?.title ?? text} · ${m.topology_connection_count({ count: node.usage.connections })} · ${usageAmount(node.usage, 'bytes')}`}
                  onMouseEnter={() => setHovered(node.id)}
                  onMouseLeave={() => setHovered(undefined)}
                  onFocus={() => setHovered(node.id)}
                  onBlur={() => setHovered(undefined)}
                  className={cn(
                    // A button centers its content by itself, but the
                    // merged node is a div: center both explicitly.
                    'absolute flex h-14 flex-col justify-center rounded-2xl px-3 text-left outline-none',
                    merged
                      ? 'cursor-default'
                      : 'focus-visible:ring-primary ring-offset-surface cursor-pointer focus-visible:ring-2 focus-visible:ring-offset-2',
                    tones[node.layer % tones.length],
                    active && 'ring-primary ring-2 ring-offset-2',
                  )}
                  {...(large
                    ? {}
                    : {
                        initial: { opacity: 0, top: position.y },
                        animate: { top: position.y, opacity: 1 },
                        exit: { opacity: 0 },
                        transition,
                      })}
                  style={{
                    left: `${(position.x / graphWidth) * 100}%`,
                    width: `${(NODE_WIDTH / graphWidth) * 100}%`,
                    ...(large && { top: position.y }),
                  }}
                >
                  <span
                    className={cn(
                      'block truncate text-xs font-medium',
                      info?.mono && 'font-mono',
                    )}
                  >
                    {text}
                  </span>
                  <span className="mt-0.5 block text-xs tabular-nums opacity-75">
                    {amount}
                  </span>
                </Node>
              )
            })}
          </Presence>
        </motion.div>
      </div>
    </div>
  )
}
