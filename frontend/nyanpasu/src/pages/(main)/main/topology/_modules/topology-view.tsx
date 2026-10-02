import { AnimatePresence, motion } from 'motion/react'
import { useMemo, useState, type ReactNode } from 'react'
import { useMedia } from 'react-use'
import { Card } from '@nyanpasu/ui/card'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { m } from '@/paraglide/messages'
import type {
  Dimension,
  Metric,
  Topology,
  TopologyNode,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import GeographyView from './geography-view'
import {
  LAYER_TEMPLATES,
  LIMITS,
  TEMPLATES,
  type LayerTemplate,
  type Limit,
  type SearchFilter,
} from './search'
import {
  dimensionName,
  largest,
  usageAmount,
  usageValue,
  type UsageLabel,
} from './usage-label'

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
// The drawing scrolls inside this height instead of stretching the page.
const MAX_GRAPH_HEIGHT = '40rem'

const limitName = (limit: Limit) =>
  limit === 'all'
    ? m.traffic_limit_all()
    : m.traffic_limit_top({ count: limit })

const templateName = (template: LayerTemplate) =>
  LAYER_TEMPLATES[template].map(dimensionName).join(' → ')

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

export default function TopologyView({
  topology,
  view: mode,
  onViewChange,
  metric,
  onMetricChange,
  template,
  onTemplateChange,
  limit,
  onLimitChange,
  filters,
  labelOf,
  onSelect,
}: {
  /** What the page's report returned for the request of the current view. */
  topology: Topology | null | undefined
  view: 'flow' | 'map'
  onViewChange: (view: 'flow' | 'map') => void
  metric: Metric
  onMetricChange: (metric: Metric) => void
  template: LayerTemplate
  onTemplateChange: (template: LayerTemplate) => void
  limit: Limit
  onLimitChange: (limit: Limit) => void
  filters: SearchFilter[]
  labelOf: (dimension: Dimension, key: string) => UsageLabel
  /** A node or region was picked: its dimension and value become a filter. */
  onSelect: (dimension: Dimension, key: string) => void
}) {
  const reducedMotion = useMedia('(prefers-reduced-motion: reduce)', false)
  const transition = {
    duration: reducedMotion ? 0 : 0.32,
    ease: [0.2, 0, 0, 1] as const,
  }
  const [hovered, setHovered] = useState<string>()
  const layers = LAYER_TEMPLATES[template]
  const graphWidth = COLUMN_STEP * (layers.length - 1) + NODE_WIDTH

  const columns = useMemo(() => {
    const result: TopologyNode[][] = layers.map(() => [])
    for (const node of topology?.nodes ?? []) result[node.layer]?.push(node)
    return result
  }, [topology, layers])
  const nodes = columns.flat()
  const nodeById = new Map(nodes.map((node) => [node.id, node]))
  const edges = (topology?.edges ?? []).filter(
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
    <section className="space-y-4" aria-label={m.topology_title()}>
      <Card className="p-4 md:p-5">
        <div className="mb-6 flex flex-wrap items-end justify-between gap-x-4 gap-y-4">
          <SegmentedButton
            value={mode}
            onValueChange={(value) => {
              if (value !== 'flow' && value !== 'map') return
              onViewChange(value)
            }}
            size="sm"
            className="w-auto shrink-0"
            aria-label={m.topology_view()}
          >
            <SegmentedButtonItem
              value="flow"
              className="flex-none whitespace-nowrap"
            >
              {m.topology_flow()}
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="map"
              className="flex-none whitespace-nowrap"
            >
              {m.topology_geo_map()}
            </SegmentedButtonItem>
          </SegmentedButton>

          <div className="flex min-w-0 flex-wrap items-end gap-x-3 gap-y-4">
            <SegmentedButton
              value={metric}
              onValueChange={(value) => {
                if (value === 'connections' || value === 'bytes')
                  onMetricChange(value)
              }}
              size="sm"
              className="w-auto shrink-0"
              aria-label={m.topology_weight()}
            >
              <SegmentedButtonItem
                value="bytes"
                className="flex-none whitespace-nowrap"
              >
                {m.topology_by_bytes()}
              </SegmentedButtonItem>
              <SegmentedButtonItem
                value="connections"
                className="flex-none whitespace-nowrap"
              >
                {m.topology_by_connections()}
              </SegmentedButtonItem>
            </SegmentedButton>

            {mode === 'flow' && (
              <div className="w-full sm:w-64">
                <Select
                  variant="outlined"
                  value={template}
                  onValueChange={(next) =>
                    onTemplateChange(next as LayerTemplate)
                  }
                >
                  <SelectTrigger
                    className="h-9 min-w-0 py-1"
                    aria-label={m.traffic_layers_label()}
                  >
                    <SelectValue
                      className="truncate pr-4 text-sm"
                      placeholder={m.traffic_layers_label()}
                    >
                      {templateName(template)}
                    </SelectValue>
                  </SelectTrigger>

                  <SelectContent className="min-w-64">
                    {TEMPLATES.map((value) => (
                      <SelectItem key={value} value={value}>
                        {templateName(value)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}

            {mode === 'flow' && (
              <div className="w-full sm:w-36">
                <Select
                  variant="outlined"
                  value={String(limit)}
                  onValueChange={(next) => {
                    const value = LIMITS.find((item) => String(item) === next)

                    if (value !== undefined) onLimitChange(value)
                  }}
                >
                  <SelectTrigger
                    className="h-9 min-w-0 py-1"
                    aria-label={m.traffic_limit_label()}
                  >
                    <SelectValue
                      className="truncate pr-4 text-sm"
                      placeholder={m.traffic_limit_label()}
                    >
                      {limitName(limit)}
                    </SelectValue>
                  </SelectTrigger>

                  <SelectContent>
                    {LIMITS.map((value) => (
                      <SelectItem key={value} value={String(value)}>
                        {limitName(value)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
          </div>
        </div>

        {mode === 'map' ? (
          <GeographyView
            topology={topology}
            metric={metric}
            selection={
              filters.find((filter) => filter.d === 'destination_region')?.v
            }
            onSelect={(code) => onSelect('destination_region', code)}
          />
        ) : !nodes.length ? (
          <div className="text-on-surface-variant grid min-h-64 place-content-center text-center">
            {topology && <p className="text-lg">{m.traffic_empty()}</p>}
          </div>
        ) : (
          <div
            // The padding leaves room for the focus ring (2px + 2px offset) of the
            // nodes at the edges, which the scroll container would clip.
            className="overflow-auto p-2"
            style={{ maxHeight: MAX_GRAPH_HEIGHT }}
            tabIndex={0}
            role="region"
            aria-label={m.topology_title()}
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
                              onClick: () => onSelect(dimension, key),
                            })}
                        aria-label={`${dimensionName(dimension)}: ${text}, ${amount}`}
                        title={`${info?.title ?? text} · ${m.topology_connection_count({ count: node.usage.connections })} · ${usageAmount(node.usage, 'bytes')}`}
                        onMouseEnter={() => setHovered(node.id)}
                        onMouseLeave={() => setHovered(undefined)}
                        onFocus={() => setHovered(node.id)}
                        onBlur={() => setHovered(undefined)}
                        className={cn(
                          'absolute h-14 rounded-2xl px-3 text-left outline-none',
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
        )}
        {mode === 'flow' && (
          <p className="text-on-surface-variant mt-3 text-xs leading-relaxed">
            {m.topology_caption()}
          </p>
        )}
      </Card>
    </section>
  )
}
