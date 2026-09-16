import { AnimatePresence, motion } from 'motion/react'
import { useMemo } from 'react'
import { useMedia } from 'react-use'
import worldMap from '@/assets/maps/world-map.json'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import parseTraffic from '@/utils/parse-traffic'
import type { ClashConnectionItem } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { buildGeography } from './geography'
import type { TopologyMetric } from './topology'

const centers: Readonly<Record<string, number[]>> = worldMap.centers
export const geographicRegions: ReadonlySet<string> = new Set(
  Object.keys(centers),
)

export function regionName(code: string) {
  return code === 'unknown'
    ? m.topology_unknown()
    : (new Intl.DisplayNames([getLocale()], { type: 'region' }).of(code) ??
        code)
}

export default function GeographyView({
  connections,
  metric,
  selection,
  onSelect,
}: {
  connections: ClashConnectionItem[]
  metric: TopologyMetric
  selection?: string
  onSelect: (code: string | undefined) => void
}) {
  const reducedMotion = useMedia('(prefers-reduced-motion: reduce)', false)
  const transition = {
    duration: reducedMotion ? 0 : 0.32,
    ease: [0.2, 0, 0, 1] as const,
  }
  const geography = useMemo(
    () => buildGeography(connections, geographicRegions),
    [connections],
  )
  const value = (item: { count: number; bytes: number }) =>
    metric === 'bytes' ? item.bytes : item.count
  const regions = [...geography.regions].sort(
    (a, b) => value(b) - value(a) || a.code.localeCompare(b.code),
  )
  const maximum = Math.max(1, ...regions.map(value))
  const unknown =
    regions.find((region) => region.code === 'unknown')?.count ?? 0
  const select = (code: string) =>
    onSelect(code === selection ? undefined : code)
  const amount = (item: { count: number; bytes: number }) =>
    metric === 'bytes'
      ? parseTraffic(item.bytes).join(' ')
      : m.topology_connection_count({ count: item.count })
  // Bound the number of animated arcs; every region remains in the list below.
  const routes = geography.routes
    .filter((route) => !selection || route.destination === selection)
    .sort((a, b) => value(b) - value(a) || a.code.localeCompare(b.code))
    .slice(0, 32)

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
        <h2 className="font-medium">{m.topology_geo_title()}</h2>
        <span className="text-on-surface-variant text-xs">
          {m.topology_geo_coverage({
            known: connections.length - unknown,
            total: connections.length,
          })}
        </span>
      </div>
      <div className="bg-secondary-container/15 relative overflow-hidden rounded-2xl">
        <svg
          viewBox="0 0 960 460"
          className="w-full"
          role="group"
          aria-label={m.topology_geo_title()}
        >
          <path
            d={worldMap.graticule}
            fill="none"
            className="stroke-outline-variant"
            strokeWidth="0.6"
            opacity="0.45"
            aria-hidden="true"
          />
          <path
            d={worldMap.outline}
            className="fill-surface-variant stroke-outline-variant"
            strokeWidth="0.5"
            aria-hidden="true"
          />
          <AnimatePresence initial={false}>
            {routes.map((route) => {
              const [x1, y1] = centers[route.source]
              const [x2, y2] = centers[route.destination]
              // A regional arc is schematic; it is not a measured network route.
              const d = `M ${x1} ${y1} Q ${(x1 + x2) / 2} ${Math.max(8, Math.min(y1, y2) - Math.min(100, Math.abs(x2 - x1) * 0.2))} ${x2} ${y2}`
              return (
                <motion.path
                  key={route.code}
                  d={d}
                  fill="none"
                  className="stroke-tertiary"
                  aria-hidden="true"
                  initial={{ opacity: 0 }}
                  animate={{
                    opacity: 0.55,
                    strokeWidth: 1 + (value(route) / maximum) * 3,
                  }}
                  exit={{ opacity: 0 }}
                  transition={transition}
                />
              )
            })}
          </AnimatePresence>
          <AnimatePresence initial={false}>
            {regions
              .filter((region) => region.code !== 'unknown')
              .map((region) => {
                const [x, y] = centers[region.code]
                const active = selection === region.code
                return (
                  <motion.g
                    key={region.code}
                    role="button"
                    tabIndex={0}
                    aria-label={`${regionName(region.code)}: ${amount(region)}`}
                    aria-pressed={active}
                    onClick={() => select(region.code)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' || event.key === ' ') {
                        event.preventDefault()
                        select(region.code)
                      }
                    }}
                    className="group cursor-pointer outline-none"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: selection && !active ? 0.35 : 1 }}
                    exit={{ opacity: 0 }}
                    transition={transition}
                  >
                    <title>
                      {regionName(region.code)} · {amount(region)}
                    </title>
                    <circle
                      cx={x}
                      cy={y}
                      r="13"
                      fill="transparent"
                      className="group-focus-visible:stroke-primary"
                      strokeWidth="2"
                    />
                    <motion.circle
                      cx={x}
                      cy={y}
                      initial={false}
                      animate={{
                        r: 4 + Math.sqrt(value(region) / maximum) * 7,
                      }}
                      transition={transition}
                      className={cn(
                        'stroke-surface',
                        active ? 'fill-tertiary' : 'fill-primary',
                      )}
                      strokeWidth="2"
                    />
                    {active && (
                      <text
                        x={x > 800 ? x - 17 : x + 17}
                        y={y - 12}
                        textAnchor={x > 800 ? 'end' : 'start'}
                        className="fill-on-surface stroke-surface text-xs font-medium"
                        strokeWidth="3"
                        paintOrder="stroke"
                      >
                        {regionName(region.code)}
                      </text>
                    )}
                  </motion.g>
                )
              })}
          </AnimatePresence>
        </svg>
        {connections.length > 0 && unknown === connections.length && (
          <p className="bg-surface/90 text-on-surface-variant absolute inset-x-4 bottom-4 rounded-2xl p-3 text-center text-sm">
            {m.topology_geo_empty()}
          </p>
        )}
      </div>
      <div
        className="flex max-h-40 flex-wrap gap-2 overflow-y-auto"
        aria-label={m.topology_geo_regions()}
      >
        {regions.map((region) => (
          <button
            key={region.code}
            type="button"
            aria-pressed={selection === region.code}
            onClick={() => select(region.code)}
            className={cn(
              'focus-visible:ring-primary rounded-full px-3 py-2 text-xs outline-none focus-visible:ring-2',
              selection === region.code
                ? 'bg-tertiary-container text-on-tertiary-container'
                : 'bg-secondary-container/50 text-on-secondary-container',
            )}
          >
            {regionName(region.code)}{' '}
            <span className="ml-2 tabular-nums opacity-75">
              {amount(region)}
            </span>
          </button>
        ))}
      </div>
      <p className="text-on-surface-variant text-xs leading-relaxed">
        {m.topology_geo_caption()}
      </p>
      <p className="text-on-surface-variant text-[10px]">
        {m.topology_geo_credit()}
      </p>
    </div>
  )
}
