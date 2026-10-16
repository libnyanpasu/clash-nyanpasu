import { cva } from 'class-variance-authority'
import { motion, type Transition } from 'motion/react'
import { memo, useMemo, useState } from 'react'
import { useMeasure } from 'react-use'
import { m } from '@/paraglide/messages'
import { dimensionName, usageAmount } from '@/utils/traffic-usage'
import type { Dimension, Topology } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useTrafficPage } from '../../_modules/traffic-context'
import { usePageTransition } from '../../_modules/transition'
import { toggleFilter } from '../../../_modules/traffic-filters'
import {
  layoutSankey,
  NODE_WIDTH,
  sankeyMinHeight,
  type SankeyBand,
} from './sankey-layout'

// Above this many nodes or edges the drawing is static: transitioning each of
// them on every hover would stutter.
const LARGE_GRAPH = 200
const LABEL_PADDING = 8
const LABEL_CHARACTER_WIDTH = 6.5
const MIN_LABEL_CHARACTERS = 6

const nodeVariants = cva('transition-opacity', {
  variants: {
    tone: {
      primary: 'fill-primary',
      tertiary: 'fill-tertiary',
      secondary: 'fill-secondary',
    },
  },
})

const TONES = ['primary', 'tertiary', 'secondary'] as const

const bandVariants = cva(
  'fill-primary opacity-20 data-dim:opacity-5 [&:not([data-static])]:transition-opacity',
)

// Each report reshapes the drawing: motion eases the shapes to their new place
// outside of React's render. Above LARGE_GRAPH they are placed at once.
const Band = memo(function Band({
  path,
  dim,
  transition,
}: {
  path: string
  dim: boolean
  transition: Transition | null
}) {
  const props = {
    className: bandVariants(),
    'data-dim': dim ? '' : undefined,
    'data-static': transition ? undefined : '',
  }

  return transition ? (
    <motion.path
      initial={false}
      animate={{ d: path }}
      transition={transition}
      {...props}
    />
  ) : (
    <path d={path} {...props} />
  )
})

const truncate = (text: string, characters: number) =>
  text.length > characters ? `${text.slice(0, characters - 1)}…` : text

/**
 * The layers as columns of nodes joined by bands as thick as their value; the
 * bands that carried traffic since the last report have a highlight running
 * along them.
 */
export function Sankey({
  layers,
  topology,
}: {
  layers: readonly Dimension[]
  topology: Topology
}) {
  const { search, labelOf, onSearchChange } = useTrafficPage()
  const { metric, filters } = search
  const [ref, { width, height: available }] = useMeasure<HTMLDivElement>()
  const [hovered, setHovered] = useState<string>()
  const { transition } = usePageTransition()

  const height = Math.max(available, sankeyMinHeight(topology.nodes))
  const large =
    topology.nodes.length > LARGE_GRAPH || topology.edges.length > LARGE_GRAPH

  const layout = useMemo(
    () =>
      layoutSankey({
        nodes: topology.nodes,
        edges: topology.edges,
        layerCount: layers.length,
        metric,
        width,
        height,
      }),
    [topology, layers.length, metric, width, height],
  )

  const lit = useMemo(() => {
    if (hovered === undefined) return null

    const ids = new Set([hovered])

    for (const { source, target } of layout.bands) {
      if (source === hovered) ids.add(target)
      if (target === hovered) ids.add(source)
    }

    return ids
  }, [hovered, layout.bands])

  const isDim = (band: SankeyBand) =>
    hovered !== undefined && band.source !== hovered && band.target !== hovered
  const nodeTransition = large ? { duration: 0 } : transition
  const labelCharacters = Math.max(
    MIN_LABEL_CHARACTERS,
    Math.floor(
      (layout.step - NODE_WIDTH - LABEL_PADDING * 2) / LABEL_CHARACTER_WIDTH,
    ),
  )

  return (
    <div
      ref={ref}
      // The drawing sits out of the flow: its height follows the container, and
      // a container that followed the drawing back would never settle. The
      // position is inline so that it holds before any stylesheet applies.
      className="min-h-64 flex-1 overflow-auto"
      style={{ position: 'relative' }}
      tabIndex={0}
      role="region"
      aria-label={m.topology_title()}
      data-slot="sankey"
    >
      {width > 0 && (
        <>
          <svg
            width={width}
            height={height}
            style={{ position: 'absolute', top: 0, left: 0 }}
            data-slot="sankey-graph"
          >
            <g aria-hidden="true" data-slot="sankey-bands">
              {layout.bands.map((band) => (
                <Band
                  key={band.id}
                  path={band.path}
                  dim={isDim(band)}
                  transition={large ? null : transition}
                />
              ))}
            </g>
            {layout.nodes.map(({ node, x, y, height: nodeHeight }) => {
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
              const last = node.layer === layers.length - 1
              const toggle = () => {
                if (key === null) return

                onSearchChange({
                  filters: toggleFilter(filters, dimension, key),
                })
              }

              return (
                <g
                  key={node.id}
                  className={cn(
                    'group transition-opacity outline-none data-dim:opacity-35',
                    merged ? 'cursor-default' : 'cursor-pointer',
                  )}
                  data-slot="sankey-node"
                  data-dim={lit && !lit.has(node.id) ? '' : undefined}
                  {...(merged
                    ? { role: 'img' }
                    : {
                        role: 'button',
                        tabIndex: 0,
                        'aria-pressed': active,
                        onClick: toggle,
                        onKeyDown: (event) => {
                          if (event.key !== 'Enter' && event.key !== ' ') return

                          event.preventDefault()
                          toggle()
                        },
                      })}
                  aria-label={`${dimensionName(dimension)}: ${text}, ${amount}`}
                  onPointerEnter={() => setHovered(node.id)}
                  onPointerLeave={() => setHovered(undefined)}
                  onFocus={() => setHovered(node.id)}
                  onBlur={() => setHovered(undefined)}
                >
                  <title>{`${info?.title ?? text} · ${m.topology_connection_count({ count: node.usage.connections })} · ${usageAmount(node.usage, 'bytes')}`}</title>
                  <motion.rect
                    x={x}
                    width={NODE_WIDTH}
                    rx={3}
                    initial={false}
                    animate={{ attrY: y, height: nodeHeight }}
                    transition={nodeTransition}
                    className={cn(
                      nodeVariants({ tone: TONES[node.layer % TONES.length] }),
                      'group-aria-pressed:stroke-on-surface group-focus-visible:stroke-secondary group-focus-visible:stroke-[3px] group-aria-pressed:stroke-2',
                    )}
                  />
                  <motion.text
                    x={
                      last ? x - LABEL_PADDING : x + NODE_WIDTH + LABEL_PADDING
                    }
                    initial={false}
                    animate={{ attrY: y + nodeHeight / 2 }}
                    transition={nodeTransition}
                    textAnchor={last ? 'end' : 'start'}
                    dominantBaseline="central"
                    className={cn(
                      'fill-on-surface stroke-surface [stroke-width:3px] text-xs font-medium select-none [paint-order:stroke] [stroke-linejoin:round]',
                      info?.mono && 'font-mono',
                      active && 'font-bold',
                    )}
                  >
                    {truncate(text, labelCharacters)}
                  </motion.text>
                </g>
              )
            })}
          </svg>
        </>
      )}
    </div>
  )
}
