import CheckRounded from '~icons/material-symbols/check-rounded'
import { AnimatePresence, motion, useInView } from 'motion/react'
import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import worldMap from '@/assets/maps/world-map.json'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import type { Metric, Topology, Usage } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { usePageTransition } from './transition'
import { largest, regionName, usageAmount, usageValue } from './usage-label'

const centers: Readonly<Record<string, number[]>> = worldMap.centers
const shapes: Readonly<Record<string, string>> = worldMap.countries
const geographicRegions: ReadonlySet<string> = new Set(Object.keys(centers))

/** Shares from which a region takes the next, darker shade. */
const SHARE_STEPS = [0.02, 0.05, 0.1, 0.25]

/**
 * How much of the theme's primary covers the land at each step, so the scale
 * follows the theme in light and dark alike.
 */
const STEP_OPACITY = [0.2, 0.4, 0.6, 0.8, 1]

const stepOf = (share: number) =>
  SHARE_STEPS.filter((threshold) => share >= threshold).length

/** A step's shade as a solid colour: the primary over the land, as on the map. */
const swatch = (step: number) =>
  `color-mix(in srgb, var(--color-primary) ${STEP_OPACITY[step] * 100}%, var(--color-surface-variant))`

const OCEAN =
  'color-mix(in srgb, var(--color-secondary-container) 45%, var(--color-surface))'

/** The projected canvas of `world-map.json`. */
const MAP_WIDTH = 960
const MAP_HEIGHT = 460

/** How long a dot takes to travel its route, whatever the route's length. */
const TRAVEL_DURATION = 2400

/** Points of a route the dot passes through, joined by straight steps. */
const TRAVEL_STEPS = 24

/** Sizes change in steps, so a refresh that barely moves a share animates nothing. */
const stepped = (size: number) => Math.round(size * 2) / 2

type Route = {
  x1: number
  y1: number
  cx: number
  cy: number
  x2: number
  y2: number
}

/** A schematic arc between two regions; it is not a measured network route. */
function routeBetween(source: string, destination: string): Route {
  const [x1, y1] = centers[source]
  const [x2, y2] = centers[destination]
  return {
    x1,
    y1,
    cx: (x1 + x2) / 2,
    cy: Math.max(8, Math.min(y1, y2) - Math.min(100, Math.abs(x2 - x1) * 0.2)),
    x2,
    y2,
  }
}

/**
 * Keyframes that carry a dot along `route` at an even pace, fading in where it
 * leaves and out where it arrives. The animated box is the map's, so its
 * percentages are map coordinates.
 */
function travel({ x1, y1, cx, cy, x2, y2 }: Route): Keyframe[] {
  const points = Array.from({ length: TRAVEL_STEPS + 1 }, (_, step) => {
    const t = step / TRAVEL_STEPS
    const u = 1 - t
    return [
      u * u * x1 + 2 * u * t * cx + t * t * x2,
      u * u * y1 + 2 * u * t * cy + t * t * y2,
    ] as const
  })

  const distances = [0]
  for (let step = 1; step < points.length; step++) {
    const [x, y] = points[step]
    const [px, py] = points[step - 1]
    distances.push(distances[step - 1] + Math.hypot(x - px, y - py))
  }
  const length = distances[TRAVEL_STEPS]

  return points.map(([x, y], step) => {
    const offset = distances[step] / length
    return {
      offset,
      transform: `translate(${(x / MAP_WIDTH) * 100}%, ${(y / MAP_HEIGHT) * 100}%)`,
      opacity: Math.min(1, offset / 0.15, (1 - offset) / 0.15),
    }
  })
}

/**
 * Where in its loop the dot of the route `key` starts: fixed per route, so the
 * dots do not move in step and reordering the routes never restarts one.
 */
const startOf = (key: string) =>
  [...key].reduce(
    (hash, char) => (hash * 31 + char.charCodeAt(0)) % TRAVEL_DURATION,
    0,
  )

/**
 * A dot travelling a route. A Web Animation on transform runs on the
 * compositor; a dot animated inside the SVG would repaint the whole map on
 * every frame and stall whenever a refresh keeps the main thread busy.
 */
function TravellingDot({
  route,
  delay,
  faded,
}: {
  route: Route
  /** How far along its loop the dot starts, in milliseconds. */
  delay: number
  faded: boolean
}) {
  const box = useRef<HTMLSpanElement>(null)

  const { x1, y1, cx, cy, x2, y2 } = route

  useEffect(() => {
    const animation = box.current?.animate(travel({ x1, y1, cx, cy, x2, y2 }), {
      duration: TRAVEL_DURATION,
      delay: -delay,
      iterations: Infinity,
      easing: 'linear',
    })

    return () => animation?.cancel()
  }, [x1, y1, cx, cy, x2, y2, delay])

  return (
    <span
      className={cn(
        'absolute inset-0 transition-opacity duration-300',
        faded && 'opacity-20',
      )}
    >
      {/* Hidden whenever the animation is not running. */}
      <span ref={box} className="absolute inset-0 opacity-0">
        <span className="bg-tertiary absolute top-0 left-0 size-1.5 -translate-1/2 rounded-full" />
      </span>
    </span>
  )
}

/** The world without traffic, rendered once: nothing in it follows the data. */
const BaseMap = memo(function BaseMap() {
  return (
    <>
      <path d={worldMap.sphere} style={{ fill: OCEAN }} />
      <path
        d={worldMap.graticule}
        fill="none"
        className="stroke-outline-variant"
        strokeWidth="0.5"
      />

      {/* Borders take the surface colour: a gap that reads in both themes. */}
      <g className="fill-surface-variant stroke-surface" strokeWidth="0.6">
        <path d={worldMap.unassigned} />
        {Object.entries(shapes).map(([code, d]) => (
          <path key={code} d={d} />
        ))}
      </g>
    </>
  )
})

type Region = {
  code: string
  usage: Usage
  /** Of the metric, over every region. */
  share: number
  /** Located by an address its outbound dialed, at least once. */
  measured: boolean
}

function ShareLegend({ percent }: { percent: (share: number) => string }) {
  return (
    <span className="flex items-center gap-3">
      {m.topology_geo_share_legend()}

      <span className="grid grid-cols-5 gap-0.5">
        {STEP_OPACITY.map((_, step) => (
          <span key={step} className="flex w-14 flex-col items-center gap-1">
            <span
              className={cn(
                'h-2 w-full',
                step === 0 && 'rounded-l-full',
                step === STEP_OPACITY.length - 1 && 'rounded-r-full',
              )}
              style={{ backgroundColor: swatch(step) }}
            />
            <span className="tabular-nums">
              {step === 0
                ? `< ${percent(SHARE_STEPS[0])}`
                : `≥ ${percent(SHARE_STEPS[step - 1])}`}
            </span>
          </span>
        ))}
      </span>
    </span>
  )
}

export default function GeographyView({
  topology,
  metric,
  selection,
  onSelect,
  measuredOnly,
  onMeasuredOnlyChange,
}: {
  /**
   * The report's source regions, destination regions and how each destination
   * was located, nothing merged.
   */
  topology: Topology | null | undefined
  metric: Metric
  selection?: string
  onSelect: (code: string) => void
  measuredOnly: boolean
  onMeasuredOnlyChange: () => void
}) {
  const { reducedMotion, transition } = usePageTransition()

  const [hovered, setHovered] = useState<string | null>(null)

  const map = useRef<HTMLDivElement>(null)

  // Off screen, the travelling dots need not keep the compositor busy.
  const onScreen = useInView(map)

  const { regions, routes, total, unknown, dialed, resolved } = useMemo(() => {
    const nodes = topology?.nodes ?? []
    const nodeOf = new Map(nodes.map((node) => [node.id, node]))

    const routes: { source: string; destination: string; usage: Usage }[] = []
    const measured = new Set<string>()
    for (const edge of topology?.edges ?? []) {
      const source = nodeOf.get(edge.source)
      const target = nodeOf.get(edge.target)
      if (!source?.key || !target?.key) {
        continue
      }
      if (source.layer === 0 && target.layer === 1) {
        if (
          source.key !== target.key &&
          geographicRegions.has(source.key) &&
          geographicRegions.has(target.key)
        ) {
          routes.push({
            source: source.key,
            destination: target.key,
            usage: edge.usage,
          })
        }
      } else if (
        source.layer === 1 &&
        target.layer === 2 &&
        target.key === 'dialed'
      ) {
        measured.add(source.key)
      }
    }

    // The destination totals are the second layer's nodes.
    const destinations = nodes.filter(
      (node) => node.layer === 1 && node.key !== null,
    )
    const sum = destinations.reduce(
      (sum, { usage }) => sum + usageValue(usage, metric),
      0,
    )
    const regions: Region[] = destinations
      .map((node) => ({
        code: node.key!,
        usage: node.usage,
        share: sum > 0 ? usageValue(node.usage, metric) / sum : 0,
        measured: measured.has(node.key!),
      }))
      .sort((a, b) => b.share - a.share || a.code.localeCompare(b.code))

    const connectionsOf = (layer: number, key: string) =>
      nodes
        .filter((node) => node.layer === layer && node.key === key)
        .reduce((sum, { usage }) => sum + usage.connections, 0)

    return {
      regions,
      routes,
      total: destinations.reduce(
        (sum, { usage }) => sum + usage.connections,
        0,
      ),
      unknown: destinations
        .filter((node) => !geographicRegions.has(node.key!))
        .reduce((sum, { usage }) => sum + usage.connections, 0),
      dialed: connectionsOf(2, 'dialed'),
      resolved: connectionsOf(2, 'resolved'),
    }
  }, [topology, metric])

  // No topology yet is a request in flight; a topology without regions is empty.
  const hint =
    topology && total === 0
      ? m.traffic_empty()
      : total > 0 && unknown === total
        ? m.topology_geo_empty()
        : undefined

  const value = (usage: Usage) => usageValue(usage, metric)
  const amount = (usage: Usage) => usageAmount(usage, metric)
  const percent = useMemo(() => {
    const whole = new Intl.NumberFormat(getLocale(), {
      style: 'percent',
      maximumFractionDigits: 0,
    })
    const fine = new Intl.NumberFormat(getLocale(), {
      style: 'percent',
      maximumFractionDigits: 1,
    })
    return (share: number) =>
      share > 0 && share < 0.1 ? fine.format(share) : whole.format(share)
  }, [])

  const located = regions.filter(({ code }) => geographicRegions.has(code))
  const maximum = largest(
    routes.map(({ usage }) => value(usage)),
    1,
  )
  // Bound the number of animated arcs; every region remains in the list.
  const arcs = routes
    .filter((route) => !selection || route.destination === selection)
    .sort(
      (a, b) =>
        value(b.usage) - value(a.usage) ||
        `${a.source}:${a.destination}`.localeCompare(
          `${b.source}:${b.destination}`,
        ),
    )
    .slice(0, 32)
  const arrivals = new Set(arcs.map((arc) => arc.destination))
  const departures = new Set(arcs.map((arc) => arc.source))
  // Hovering a region brings its routes forward.
  const touches = (arc: (typeof arcs)[number]) =>
    !hovered || arc.source === hovered || arc.destination === hovered

  const label = (region: Region) =>
    [
      regionName(region.code),
      amount(region.usage),
      percent(region.share),
      ...(region.measured ? [] : [m.topology_geo_basis_resolved()]),
    ].join(' · ')

  // The map is a pointer shortcut; the list is the accessible control.
  const pointer = (code: string) => ({
    onClick: () => onSelect(code),
    onPointerEnter: () => setHovered(code),
    onPointerLeave: () => setHovered(null),
    className: 'cursor-pointer',
  })

  const marked = [...new Set([selection, hovered])].filter(
    (code): code is string => !!code && geographicRegions.has(code),
  )
  // A selected destination shows where its traffic comes from as well.
  const sources = selection
    ? [...departures].filter((code) => !marked.includes(code))
    : []
  const highlights = [
    ...marked.map((code) => ({ code, source: false })),
    ...sources.map((code) => ({ code, source: true })),
  ]

  return (
    <div className="@container space-y-4" data-slot="traffic-geography">
      <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
        <h2 className="font-medium">{m.topology_geo_title()}</h2>

        <div className="flex flex-wrap items-center gap-3">
          <span className="text-on-surface-variant text-xs tabular-nums">
            {m.topology_geo_coverage({ dialed, resolved, total })}
          </span>

          <button
            type="button"
            aria-pressed={measuredOnly}
            onClick={onMeasuredOnlyChange}
            className={cn(
              'flex h-8 cursor-pointer items-center gap-2 rounded-lg border px-3 text-sm outline-none',
              'focus-visible:ring-primary transition-colors focus-visible:ring-2',
              measuredOnly
                ? 'bg-secondary-container text-on-secondary-container border-transparent'
                : 'border-outline-variant text-on-surface-variant hover:bg-on-surface/8',
            )}
            data-slot="traffic-geography-measured-only"
          >
            {measuredOnly && <CheckRounded className="-ml-1 size-4" />}
            {m.topology_geo_dialed_only()}
          </button>
        </div>
      </div>

      <div className="grid gap-4 @5xl:grid-cols-[minmax(0,1fr)_20rem]">
        <div
          ref={map}
          className="relative overflow-hidden rounded-2xl"
          data-slot="traffic-geography-map"
        >
          <svg
            viewBox={`0 0 ${MAP_WIDTH} ${MAP_HEIGHT}`}
            className="block w-full"
            role="img"
            aria-label={m.topology_geo_title()}
          >
            <defs>
              <pattern
                id="traffic-geography-hatch"
                width="4"
                height="4"
                patternUnits="userSpaceOnUse"
                patternTransform="rotate(45)"
              >
                <rect
                  width="1.5"
                  height="4"
                  className="fill-surface"
                  opacity="0.7"
                />
              </pattern>
              {/* Its tip stops short of the destination's dot. */}
              <marker
                id="traffic-geography-arrow"
                viewBox="0 0 10 10"
                refX="16"
                refY="5"
                markerWidth="7"
                markerHeight="7"
                markerUnits="userSpaceOnUse"
                orient="auto"
              >
                <path d="M 0 0 L 10 5 L 0 10 z" className="fill-tertiary" />
              </marker>
            </defs>

            <BaseMap />

            <AnimatePresence initial={false}>
              {located
                .filter((region) => shapes[region.code])
                .map((region) => (
                  <motion.g
                    key={region.code}
                    {...pointer(region.code)}
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={transition}
                  >
                    <title>{label(region)}</title>
                    <motion.path
                      d={shapes[region.code]}
                      className="fill-primary stroke-surface"
                      strokeWidth="0.6"
                      initial={false}
                      animate={{
                        fillOpacity: STEP_OPACITY[stepOf(region.share)],
                      }}
                      transition={transition}
                    />
                    <AnimatePresence initial={false}>
                      {!region.measured && (
                        <motion.path
                          d={shapes[region.code]}
                          fill="url(#traffic-geography-hatch)"
                          initial={{ opacity: 0 }}
                          animate={{ opacity: 1 }}
                          exit={{ opacity: 0 }}
                          transition={transition}
                        />
                      )}
                    </AnimatePresence>
                  </motion.g>
                ))}
            </AnimatePresence>

            <AnimatePresence initial={false}>
              {arcs.map((route) => {
                const { x1, y1, cx, cy, x2, y2 } = routeBetween(
                  route.source,
                  route.destination,
                )
                const gradient = `traffic-geography-route-${route.source}-${route.destination}`
                return (
                  <motion.g
                    key={`${route.source}:${route.destination}`}
                    className="pointer-events-none"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: touches(route) ? 1 : 0.2 }}
                    exit={{ opacity: 0 }}
                    transition={transition}
                  >
                    {/* Faint where traffic leaves, full where it arrives. */}
                    <defs>
                      <linearGradient
                        id={gradient}
                        gradientUnits="userSpaceOnUse"
                        x1={x1}
                        y1={y1}
                        x2={x2}
                        y2={y2}
                      >
                        <stop
                          offset="0"
                          stopOpacity="0.15"
                          style={{ stopColor: 'var(--color-tertiary)' }}
                        />
                        <stop
                          offset="1"
                          stopOpacity="0.9"
                          style={{ stopColor: 'var(--color-tertiary)' }}
                        />
                      </linearGradient>
                    </defs>
                    <motion.path
                      d={`M ${x1} ${y1} Q ${cx} ${cy} ${x2} ${y2}`}
                      fill="none"
                      stroke={`url(#${gradient})`}
                      strokeLinecap="round"
                      markerEnd="url(#traffic-geography-arrow)"
                      initial={false}
                      animate={{
                        strokeWidth: stepped(
                          1 + (value(route.usage) / maximum) * 3,
                        ),
                      }}
                      transition={transition}
                    />
                  </motion.g>
                )
              })}
            </AnimatePresence>

            {/* Regions too small for the outline take a shaded marker. */}
            <AnimatePresence initial={false}>
              {located
                .filter((region) => !shapes[region.code])
                .map((region) => {
                  const [x, y] = centers[region.code]
                  const r = stepped(4 + Math.sqrt(region.share) * 6)
                  return (
                    <motion.g
                      key={region.code}
                      {...pointer(region.code)}
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                      transition={transition}
                    >
                      <title>{label(region)}</title>
                      <motion.circle
                        cx={x}
                        cy={y}
                        className="fill-surface-variant"
                        initial={false}
                        animate={{ r }}
                        transition={transition}
                      />
                      <motion.circle
                        cx={x}
                        cy={y}
                        className="fill-primary stroke-surface"
                        strokeWidth="1.5"
                        initial={false}
                        animate={{
                          r,
                          fillOpacity: STEP_OPACITY[stepOf(region.share)],
                        }}
                        transition={transition}
                      />
                      {!region.measured && (
                        <circle
                          cx={x}
                          cy={y}
                          r={r}
                          fill="url(#traffic-geography-hatch)"
                        />
                      )}
                    </motion.g>
                  )
                })}
            </AnimatePresence>

            {/* The routes' ends: a ring where traffic leaves, a dot where it arrives. */}
            <AnimatePresence initial={false}>
              {[...new Set([...departures, ...arrivals])].map((code) => {
                const [x, y] = centers[code]
                const arriving = arrivals.has(code)
                return (
                  <motion.circle
                    key={`end:${code}`}
                    cx={x}
                    cy={y}
                    r={arriving ? 3.5 : 4}
                    className={cn(
                      'pointer-events-none',
                      arriving
                        ? 'fill-tertiary stroke-surface'
                        : 'fill-surface stroke-tertiary',
                    )}
                    strokeWidth={arriving ? 1.5 : 2}
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={transition}
                  />
                )
              })}
            </AnimatePresence>

            <AnimatePresence initial={false}>
              {highlights.map(({ code, source }) => {
                const [x, y] = centers[code]
                const stroke = source ? 'stroke-tertiary' : 'stroke-on-surface'
                return (
                  <motion.g
                    key={`mark:${code}`}
                    className="pointer-events-none"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={transition}
                  >
                    {shapes[code] ? (
                      <path
                        d={shapes[code]}
                        fill="none"
                        className={stroke}
                        strokeWidth="1.2"
                      />
                    ) : (
                      <circle
                        cx={x}
                        cy={y}
                        r="11"
                        fill="none"
                        className={stroke}
                        strokeWidth="1.2"
                      />
                    )}
                    {(source || code === selection) && (
                      <text
                        x={x > 800 ? x - 14 : x + 14}
                        y={y - 10}
                        textAnchor={x > 800 ? 'end' : 'start'}
                        className={cn(
                          'stroke-surface text-xs font-medium',
                          source ? 'fill-tertiary' : 'fill-on-surface',
                        )}
                        strokeWidth="3"
                        paintOrder="stroke"
                      >
                        {regionName(code)}
                      </text>
                    )}
                  </motion.g>
                )
              })}
            </AnimatePresence>
          </svg>

          {!reducedMotion && onScreen && (
            <div aria-hidden className="pointer-events-none absolute inset-0">
              {arcs.map((route) => {
                const key = `${route.source}:${route.destination}`
                return (
                  <TravellingDot
                    key={key}
                    route={routeBetween(route.source, route.destination)}
                    delay={startOf(key)}
                    faded={!touches(route)}
                  />
                )
              })}
            </div>
          )}

          {hint && (
            <p className="bg-surface/90 text-on-surface-variant absolute inset-x-4 bottom-4 rounded-2xl p-3 text-center text-sm">
              {hint}
            </p>
          )}
        </div>

        {/* Beside a wide map it takes the map's height and scrolls; below, it lists all. */}
        <div className="relative min-h-0">
          <div className="flex flex-col gap-2 @5xl:absolute @5xl:inset-0">
            <p className="text-on-surface-variant px-4 text-xs">
              {m.topology_geo_region_count({ count: regions.length })}
            </p>

            <ScrollArea className="min-h-0 flex-1">
              <div
                className="flex flex-col gap-0.5"
                aria-label={m.topology_geo_regions()}
                data-slot="traffic-geography-regions"
              >
                <AnimatePresence initial={false}>
                  {regions.map((region, index) => {
                    const geographic = geographicRegions.has(region.code)
                    const active = selection === region.code
                    return (
                      <motion.button
                        key={region.code}
                        // Measured only when its place changes, not on every refresh.
                        layout="position"
                        layoutDependency={index}
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        transition={transition}
                        type="button"
                        aria-pressed={active}
                        aria-label={label(region)}
                        onClick={() => onSelect(region.code)}
                        onPointerEnter={() => setHovered(region.code)}
                        onPointerLeave={() => setHovered(null)}
                        className={cn(
                          'flex w-full min-w-0 cursor-pointer flex-col gap-1 rounded-xl px-4 py-2 text-left outline-none',
                          'focus-visible:ring-primary transition-colors focus-visible:ring-2',
                          'hover:bg-on-surface/8',
                          active && 'bg-secondary-container',
                          !active &&
                            hovered === region.code &&
                            'bg-on-surface/8',
                        )}
                      >
                        <span className="flex min-w-0 items-center gap-2 text-sm">
                          <span
                            aria-hidden
                            className={cn(
                              'size-2.5 shrink-0 rounded-full',
                              !geographic && 'bg-outline-variant',
                            )}
                            style={
                              geographic
                                ? {
                                    backgroundColor: swatch(
                                      stepOf(region.share),
                                    ),
                                  }
                                : undefined
                            }
                          />

                          <span
                            className="min-w-0 flex-1 truncate font-medium"
                            title={regionName(region.code)}
                          >
                            {regionName(region.code)}
                          </span>

                          {geographic && !region.measured && (
                            <span className="bg-surface-variant text-on-surface-variant shrink-0 rounded-md px-1.5 text-[10px] leading-4">
                              {m.topology_geo_basis_resolved()}
                            </span>
                          )}

                          <span className="text-on-surface-variant shrink-0 text-xs tabular-nums">
                            {amount(region.usage)}
                          </span>

                          <span className="w-12 shrink-0 text-right font-medium tabular-nums">
                            {percent(region.share)}
                          </span>
                        </span>

                        <span className="bg-surface-variant block h-1 overflow-hidden rounded-full">
                          <motion.span
                            className={cn(
                              'block h-full rounded-full',
                              geographic ? 'bg-primary' : 'bg-outline',
                            )}
                            initial={false}
                            animate={{ width: `${region.share * 100}%` }}
                            transition={transition}
                          />
                        </span>
                      </motion.button>
                    )
                  })}
                </AnimatePresence>
              </div>
            </ScrollArea>
          </div>
        </div>
      </div>

      <div className="text-on-surface-variant flex flex-wrap items-center gap-x-8 gap-y-2 text-xs">
        <ShareLegend percent={percent} />

        <span className="flex items-center gap-2">
          <svg className="size-4 rounded-sm" viewBox="0 0 16 16" aria-hidden>
            <rect width="16" height="16" style={{ fill: swatch(2) }} />
            <rect width="16" height="16" fill="url(#traffic-geography-hatch)" />
          </svg>
          {m.topology_geo_resolved_legend()}
        </span>
      </div>

      {routes.length === 0 && located.length > 0 && (
        <p className="text-on-surface-variant text-xs">
          {m.topology_geo_no_routes()}
        </p>
      )}

      <p className="text-on-surface-variant text-xs leading-relaxed">
        {m.topology_geo_caption()}
      </p>

      <p className="text-on-surface-variant text-[10px]">
        {m.topology_geo_credit()}
      </p>
    </div>
  )
}
