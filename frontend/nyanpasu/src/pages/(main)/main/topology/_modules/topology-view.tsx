import PauseRounded from '~icons/material-symbols/pause-rounded'
import PlayArrowRounded from '~icons/material-symbols/play-arrow-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { useMedia } from 'react-use'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import type { ClashConnectionItem } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { connectionRegion } from './geography'
import GeographyView, { geographicRegions, regionName } from './geography-view'
import {
  buildTopology,
  type TopologyMetric,
  type TopologyNode,
} from './topology'

const tones = [
  'bg-primary-container text-on-primary-container',
  'bg-secondary-container text-on-secondary-container',
  'bg-tertiary-container text-on-tertiary-container',
  'bg-primary-container text-on-primary-container',
]
const colors = ['primary', 'secondary', 'tertiary', 'primary']
const traffic = (bytes: number) => parseTraffic(bytes).join(' ')
const NODE_WIDTH = 190
const COLUMN_STEP = 270
const ROW_STEP = 76
const GRAPH_WIDTH = COLUMN_STEP * 3 + NODE_WIDTH

export default function TopologyView({
  connections,
  filterKey,
  isLoading,
  error,
}: {
  connections: ClashConnectionItem[]
  filterKey?: string
  isLoading: boolean
  error: unknown
}) {
  const reducedMotion = useMedia('(prefers-reduced-motion: reduce)', false)
  const transition = {
    duration: reducedMotion ? 0 : 0.32,
    ease: [0.2, 0, 0, 1] as const,
  }
  const [mode, setMode] = useState('flow')
  const [country, setCountry] = useState<string>()
  const [metric, setMetric] = useState<TopologyMetric>('connections')
  const [frozen, setFrozen] = useState<ClashConnectionItem[]>()
  const [selection, setSelection] = useState<string>()
  useEffect(() => {
    setFrozen(undefined)
    setSelection(undefined)
    setCountry(undefined)
  }, [filterKey])
  const displayed = frozen ?? connections
  const graph = useMemo(
    () => buildTopology(displayed, metric),
    [displayed, metric],
  )
  const nodes = graph.layers.flat()
  const selected = nodes.find((node) => node.id === selection)
  const selectedCountry =
    mode === 'map' &&
    country &&
    displayed.some(
      (connection) =>
        connectionRegion(connection, 'destination', geographicRegions) ===
        country,
    )
      ? country
      : undefined
  const matching = selectedCountry
    ? displayed.filter(
        (connection) =>
          connectionRegion(connection, 'destination', geographicRegions) ===
          selectedCountry,
      )
    : selected
      ? displayed.filter((connection) =>
          selected.connectionIds.has(connection.id),
        )
      : displayed
  const sorted = [...matching].sort(
    (a, b) => b.upload + b.download - a.upload - a.download,
  )
  const totals = displayed.reduce(
    (total, connection) => ({
      upload: total.upload + Math.max(0, connection.upload),
      download: total.download + Math.max(0, connection.download),
    }),
    { upload: 0, download: 0 },
  )
  const headings = [
    m.topology_source(),
    m.topology_rule(),
    m.topology_chain(),
    m.topology_outbound(),
  ]
  const label = (node: TopologyNode) =>
    node.other
      ? m.topology_other()
      : (node.label ??
        (node.layer === 2 ? m.topology_no_group() : m.topology_unknown()))
  const amount = (item: { count: number; bytes: number }) =>
    metric === 'bytes'
      ? traffic(item.bytes)
      : m.topology_connection_count({ count: item.count })
  const height =
    Math.max(3, ...graph.layers.map((layer) => layer.length)) * ROW_STEP
  const positions = new Map(
    graph.layers.flatMap((layer, column) =>
      layer.map(
        (node, row) =>
          [
            node.id,
            {
              x: column * COLUMN_STEP,
              y: row * ROW_STEP + (height - layer.length * ROW_STEP) / 2,
            },
          ] as const,
      ),
    ),
  )
  const maximum = Math.max(
    1,
    ...graph.links.map((link) =>
      metric === 'bytes' ? link.bytes : link.count,
    ),
  )
  const relevant = (ids: Set<string>) =>
    !selected || [...ids].some((id) => selected.connectionIds.has(id))

  return (
    <section
      className="text-on-surface space-y-5 p-4 md:p-6"
      aria-label={m.topology_title()}
    >
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div className="space-y-1">
          <div className="text-primary text-xs font-medium tracking-widest uppercase">
            {m.topology_eyebrow()}
          </div>
          <h1 className="text-2xl font-medium">{m.topology_title()}</h1>
          <p className="text-on-surface-variant max-w-xl text-sm">
            {m.topology_description()}
          </p>
        </div>
        <Button
          variant="stroked"
          className="flex items-center gap-2"
          aria-pressed={!!frozen}
          onClick={() => setFrozen(frozen ? undefined : [...connections])}
        >
          {frozen ? (
            <PlayArrowRounded className="size-5" />
          ) : (
            <PauseRounded className="size-5" />
          )}
          {frozen ? m.topology_resume() : m.topology_pause()}
        </Button>
      </header>

      {error ? (
        <p
          role="status"
          className="bg-error-container text-on-error-container rounded-2xl p-4 text-sm"
        >
          {m.topology_unavailable()}
        </p>
      ) : null}

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        {[
          [m.topology_active(), displayed.length.toLocaleString()],
          [m.topology_download(), traffic(totals.download)],
          [m.topology_upload(), traffic(totals.upload)],
        ].map(([title, value], index) => (
          <div key={title} className={cn('rounded-3xl p-5', tones[index])}>
            <p className="text-sm opacity-80">{title}</p>
            <p className="mt-2 text-3xl font-medium tabular-nums">{value}</p>
          </div>
        ))}
      </div>

      <Card variant="outline" className="p-4 md:p-5">
        <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2 text-sm font-medium">
            <span
              className={cn(
                'size-2 rounded-full',
                frozen ? 'bg-outline' : 'bg-primary',
              )}
            />
            {frozen ? m.topology_paused() : m.topology_live()}
          </div>
          <SegmentedButton
            value={mode}
            onValueChange={(value) => {
              if (!value) return
              setMode(value)
              setSelection(undefined)
              setCountry(undefined)
            }}
            size="sm"
            variant="tabs"
            className="w-60 max-w-full"
            aria-label={m.topology_view()}
          >
            <SegmentedButtonItem value="flow">
              {m.topology_flow()}
            </SegmentedButtonItem>
            <SegmentedButtonItem value="map">
              {m.topology_geo_map()}
            </SegmentedButtonItem>
          </SegmentedButton>
          <SegmentedButton
            value={metric}
            onValueChange={(value) => {
              if (value === 'connections' || value === 'bytes') setMetric(value)
            }}
            size="sm"
            className="w-52 max-w-full"
            aria-label={m.topology_weight()}
          >
            <SegmentedButtonItem value="connections">
              {m.topology_by_connections()}
            </SegmentedButtonItem>
            <SegmentedButtonItem value="bytes">
              {m.topology_by_bytes()}
            </SegmentedButtonItem>
          </SegmentedButton>
        </div>

        {mode === 'map' ? (
          <GeographyView
            connections={displayed}
            metric={metric}
            selection={selectedCountry}
            onSelect={setCountry}
          />
        ) : !displayed.length ? (
          <div className="text-on-surface-variant grid min-h-64 place-content-center text-center">
            <p className="text-lg">
              {isLoading ? m.topology_loading() : m.connections_empty_message()}
            </p>
            <p className="mt-2 text-sm">{m.topology_empty_hint()}</p>
          </div>
        ) : (
          <div
            className="overflow-x-auto pb-2"
            tabIndex={0}
            role="region"
            aria-label={m.topology_title()}
          >
            <div className="min-w-[1000px]">
              <div className="mb-3 flex justify-between text-xs font-medium tracking-wide">
                {headings.map((heading, layer) => (
                  <div
                    key={heading}
                    style={{ width: `${(NODE_WIDTH / GRAPH_WIDTH) * 100}%` }}
                    className="text-on-surface-variant flex gap-2 px-3"
                  >
                    <span className="text-outline">0{layer + 1}</span>
                    {heading}
                  </div>
                ))}
              </div>
              <motion.div
                className="relative"
                initial={false}
                animate={{ height }}
                transition={transition}
              >
                <motion.svg
                  initial={false}
                  animate={{ viewBox: `0 0 ${GRAPH_WIDTH} ${height}` }}
                  transition={transition}
                  preserveAspectRatio="none"
                  className="pointer-events-none absolute inset-0 size-full"
                  aria-hidden="true"
                >
                  <AnimatePresence initial={false}>
                    {graph.links.map((link) => {
                      const start = positions.get(link.source)!
                      const end = positions.get(link.target)!
                      const x = start.x + NODE_WIDTH
                      const y = start.y + 28
                      const value = metric === 'bytes' ? link.bytes : link.count
                      if (!value) return null
                      return (
                        <motion.path
                          key={JSON.stringify([link.source, link.target])}
                          d={`M ${x} ${y} C ${x + 40} ${y}, ${end.x - 40} ${end.y + 28}, ${end.x} ${end.y + 28}`}
                          fill="none"
                          stroke={`var(--color-md-${colors[Math.round(start.x / COLUMN_STEP)]})`}
                          initial={{ opacity: 0 }}
                          animate={{
                            d: `M ${x} ${y} C ${x + 40} ${y}, ${end.x - 40} ${end.y + 28}, ${end.x} ${end.y + 28}`,
                            strokeWidth: 1 + (value / maximum) * 13,
                            opacity: relevant(link.connectionIds)
                              ? selected
                                ? 0.65
                                : 0.25
                              : 0.04,
                          }}
                          exit={{ opacity: 0 }}
                          transition={transition}
                        />
                      )
                    })}
                  </AnimatePresence>
                </motion.svg>
                <AnimatePresence initial={false}>
                  {nodes.map((node) => {
                    const position = positions.get(node.id)!
                    const active = selected?.id === node.id
                    return (
                      <motion.button
                        key={node.id}
                        type="button"
                        aria-pressed={active}
                        aria-label={`${headings[node.layer]}: ${label(node)}, ${amount(node)}`}
                        title={`${label(node)} · ${m.topology_connection_count({ count: node.count })} · ${traffic(node.bytes)}`}
                        onClick={() =>
                          setSelection(active ? undefined : node.id)
                        }
                        className={cn(
                          'focus-visible:ring-primary ring-offset-surface absolute h-14 cursor-pointer rounded-2xl px-3 text-left outline-none focus-visible:ring-2 focus-visible:ring-offset-2',
                          tones[node.layer],
                          active && 'ring-primary ring-2 ring-offset-2',
                        )}
                        initial={{ opacity: 0, top: position.y }}
                        animate={{
                          top: position.y,
                          opacity: relevant(node.connectionIds) ? 1 : 0.3,
                        }}
                        exit={{ opacity: 0 }}
                        transition={transition}
                        style={{
                          left: `${(position.x / GRAPH_WIDTH) * 100}%`,
                          width: `${(NODE_WIDTH / GRAPH_WIDTH) * 100}%`,
                        }}
                      >
                        <span className="block truncate text-xs font-medium">
                          {label(node)}
                        </span>
                        <span className="mt-0.5 block text-xs tabular-nums opacity-75">
                          {amount(node)}
                        </span>
                      </motion.button>
                    )
                  })}
                </AnimatePresence>
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

      <Card variant="outline" className="p-5">
        <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
          <div className="min-w-0">
            <h2 className="truncate font-medium">
              {selectedCountry
                ? regionName(selectedCountry)
                : selected
                  ? label(selected)
                  : m.topology_details()}
            </h2>
            <p className="text-on-surface-variant mt-1 text-xs">
              {m.topology_detail_count({ count: matching.length })}
            </p>
          </div>
          {(selected || selectedCountry) && (
            <Button
              variant="flat"
              onClick={() => {
                setSelection(undefined)
                setCountry(undefined)
              }}
            >
              {m.topology_clear()}
            </Button>
          )}
        </div>
        <div className="divide-outline-variant/50 divide-y">
          {sorted.slice(0, 8).map((connection) => (
            <div
              key={connection.id}
              className="flex items-center justify-between gap-4 py-3"
            >
              <div className="min-w-0">
                <p
                  className="truncate text-sm"
                  title={
                    connection.metadata?.host ||
                    connection.metadata?.destinationIP
                  }
                >
                  {connection.metadata?.host ||
                    connection.metadata?.destinationIP ||
                    m.topology_unknown()}
                </p>
                <p className="text-on-surface-variant mt-1 truncate text-xs">
                  {connection.metadata?.process ||
                    connection.metadata?.sourceIP ||
                    m.topology_unknown()}{' '}
                  ·{' '}
                  {connection.chains.slice().reverse().join(' → ') ||
                    m.topology_unknown()}
                </p>
              </div>
              <span className="shrink-0 text-sm tabular-nums">
                {traffic(connection.upload + connection.download)}
              </span>
            </div>
          ))}
        </div>
      </Card>
    </section>
  )
}
