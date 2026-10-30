import AddRounded from '~icons/material-symbols/add-rounded'
import FitScreenRounded from '~icons/material-symbols/fit-screen-rounded'
import RemoveRounded from '~icons/material-symbols/remove-rounded'
import { cva } from 'class-variance-authority'
import { useEffect, useMemo, useRef, useState } from 'react'
import type { ProfileLookup } from '@/components/profile-label'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { RowAction } from '../../_modules/compact-controls'
import { layoutGraph, NODE_HEIGHT, NODE_WIDTH, traceOf } from './inspect-graph'
import { stageLabel, statusLabel, stepHeadline } from './inspect-labels'
import { layoutLanes } from './inspect-lanes'
import type { Step, StepStatus } from './inspect-steps'
import OnlyChangedSwitch from './only-changed-switch'
import StatusIcon from './status-icon'

const ZOOM_STEP = 0.1
const MIN_ZOOM = 0.4
const MAX_ZOOM = 1.6
const LEGEND: readonly StepStatus[] = [
  'start',
  'changed',
  'unchanged',
  'warn',
  'error',
]

const nodeVariants = cva(
  [
    'group/node absolute flex items-center gap-2.5 rounded-xl px-3 text-left',
    'transition-opacity outline-none',
    'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary',
    'before:pointer-events-none before:absolute before:inset-0 before:rounded-[inherit]',
    'before:bg-on-surface/8 before:opacity-0 hover:before:opacity-100',
  ],
  {
    variants: {
      state: {
        selected: 'bg-primary text-on-primary',
        trace: 'bg-primary-container text-on-primary-container',
        idle: 'bg-surface-variant text-on-surface',
      },
      dimmed: { true: 'opacity-40', false: '' },
    },
    defaultVariants: { state: 'idle', dimmed: false },
  },
)

const edgeVariants = cva('fill-none', {
  variants: {
    tone: {
      selected: 'stroke-primary stroke-[2.5]',
      trace: 'stroke-outline stroke-2',
      idle: 'stroke-outline-variant stroke-2',
    },
  },
  defaultVariants: { tone: 'idle' },
})

const clampZoom = (zoom: number) =>
  Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round(zoom * 10) / 10))

/**
 * The same steps as the rail, drawn as the graph they form: one band per
 * stage, one column per lane, and an edge for every step that feeds another.
 */
export default function StepGraph({
  steps,
  allSteps,
  selectedKey,
  onSelect,
  profiles,
  onlyChanged,
  onOnlyChangedChange,
  matchedKeys,
  className,
}: {
  /** The steps the graph shows, after the "only changed" filter. */
  steps: Step[]
  /** Every step, whatever the filter, so a hidden step is bypassed by its edges. */
  allSteps: Step[]
  selectedKey?: string
  onSelect: (key: string) => void
  profiles: ProfileLookup
  onlyChanged: boolean
  onOnlyChangedChange: (onlyChanged: boolean) => void
  /** The steps that match the field query; absent when there is no query. */
  matchedKeys?: ReadonlySet<string>
  className?: string
}) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const [viewportWidth, setViewportWidth] = useState(0)
  // Absent while the graph is fitted to the width.
  const [zoom, setZoom] = useState<number>()

  const geometry = useMemo(
    () => layoutGraph(steps, layoutLanes(allSteps, steps)),
    [allSteps, steps],
  )
  const trace = useMemo(
    () =>
      selectedKey ? traceOf(geometry.edges, selectedKey) : new Set<string>(),
    [geometry.edges, selectedKey],
  )
  const at = new Map(geometry.nodes.map((node) => [node.key, node]))

  useEffect(() => {
    const viewport = viewportRef.current
    if (!viewport) return
    const observer = new ResizeObserver(([entry]) =>
      setViewportWidth(entry.contentRect.width),
    )
    observer.observe(viewport)
    return () => observer.disconnect()
  }, [])

  const fit = viewportWidth
    ? Math.min(1, Math.max(MIN_ZOOM, viewportWidth / geometry.width))
    : 1
  const scale = zoom ?? fit

  // Keep the chosen step in sight, such as one a search picked.
  useEffect(() => {
    viewportRef.current
      ?.querySelector('[aria-current=step]')
      ?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' })
  }, [selectedKey, steps])

  return (
    <div
      data-slot="inspect-step-graph"
      className={cn(
        'bg-pane relative flex min-h-0 min-w-0 flex-col',
        className,
      )}
    >
      <div
        ref={viewportRef}
        role="group"
        aria-label={m.inspect_steps()}
        className="min-h-0 flex-1 overflow-auto"
      >
        <div
          className="relative"
          style={{
            width: geometry.width * scale,
            height: geometry.height * scale,
          }}
        >
          <div
            className="absolute top-0 left-0 origin-top-left"
            style={{
              width: geometry.width,
              height: geometry.height,
              transform: `scale(${scale})`,
            }}
          >
            {geometry.bands.map((band) => (
              <div
                key={band.stage}
                data-stage={band.stage}
                className="bg-surface absolute rounded-3xl"
                style={{
                  left: band.x,
                  top: band.y,
                  width: band.width,
                  height: band.height,
                }}
              >
                <h3 className="text-on-surface-variant px-6 pt-4 text-[11px] leading-4 font-bold tracking-[0.06em] uppercase">
                  {stageLabel(band.stage)}
                </h3>
              </div>
            ))}

            <svg
              aria-hidden
              className="pointer-events-none absolute inset-0"
              width={geometry.width}
              height={geometry.height}
            >
              {geometry.edges.map((edge) => {
                const touches =
                  edge.from === selectedKey || edge.to === selectedKey
                const tone = touches
                  ? 'selected'
                  : trace.has(edge.from) && trace.has(edge.to)
                    ? 'trace'
                    : 'idle'

                return (
                  <path
                    key={`${edge.from}-${edge.to}`}
                    d={edge.d}
                    className={edgeVariants({ tone })}
                  />
                )
              })}
            </svg>

            {steps.map((step) => {
              const node = at.get(step.key)!
              const selected = step.key === selectedKey
              const matched = matchedKeys?.has(step.key) ?? false

              return (
                <button
                  key={step.key}
                  type="button"
                  aria-current={selected ? 'step' : undefined}
                  data-key={step.key}
                  data-status={step.status}
                  className={nodeVariants({
                    state: selected
                      ? 'selected'
                      : trace.has(step.key)
                        ? 'trace'
                        : 'idle',
                    dimmed: selectedKey
                      ? !trace.has(step.key)
                      : matchedKeys
                        ? !matched
                        : false,
                  })}
                  style={{
                    left: node.x,
                    top: node.y,
                    width: NODE_WIDTH,
                    height: NODE_HEIGHT,
                  }}
                  onClick={() => onSelect(step.key)}
                >
                  <StatusIcon status={step.status} />

                  <span className="truncate text-sm leading-5 font-medium">
                    {stepHeadline(step, profiles)}
                  </span>
                </button>
              )
            })}
          </div>
        </div>
      </div>

      <div
        data-slot="inspect-graph-controls"
        className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 px-3 pt-1 pb-3"
      >
        <div className="flex items-center">
          <RowAction
            label={m.inspect_graph_zoom_out()}
            className="size-9"
            disabled={scale <= MIN_ZOOM}
            onClick={() => setZoom(clampZoom(scale - ZOOM_STEP))}
          >
            <RemoveRounded className="size-5" aria-hidden />
          </RowAction>

          <RowAction
            label={m.inspect_graph_zoom_in()}
            className="size-9"
            disabled={scale >= MAX_ZOOM}
            onClick={() => setZoom(clampZoom(scale + ZOOM_STEP))}
          >
            <AddRounded className="size-5" aria-hidden />
          </RowAction>

          <RowAction
            label={m.inspect_graph_zoom_fit()}
            className="size-9"
            disabled={zoom === undefined}
            onClick={() => setZoom(undefined)}
          >
            <FitScreenRounded className="size-5" aria-hidden />
          </RowAction>
        </div>

        <ul
          data-slot="inspect-graph-legend"
          className="text-on-surface-variant flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"
        >
          {LEGEND.map((status) => (
            <li key={status} className="flex items-center gap-1">
              <StatusIcon status={status} className="size-4" />
              {statusLabel(status)}
            </li>
          ))}
        </ul>

        <OnlyChangedSwitch
          className="ml-auto"
          checked={onlyChanged}
          onCheckedChange={onOnlyChangedChange}
        />
      </div>
    </div>
  )
}
