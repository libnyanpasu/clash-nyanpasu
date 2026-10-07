import ArrowDownwardAltRounded from '~icons/material-symbols/arrow-downward-alt-rounded'
import ArrowUpwardAltRounded from '~icons/material-symbols/arrow-upward-alt-rounded'
import { filesize } from 'filesize'
import { ReactNode, useEffect, useRef, useState } from 'react'
import { ActionSwap } from '@nyanpasu/ui/action-swap-text'
import { m } from '@/paraglide/messages'
import { useGroupTrafficSpeed } from '../../_modules/hooks'

const DEFAULT_ROTATE_MS = 3000

/** How much of the group's members are reachable and where it resolves. */
export function GroupStatus({
  available,
  total,
  chain,
}: {
  available: number
  total: number
  chain: string[]
}) {
  const chainText = chain.join(' › ')

  return (
    <span
      className="text-on-surface-variant flex min-w-0 items-center gap-1.5 text-xs whitespace-nowrap"
      data-slot="proxies-group-status"
    >
      <span className="text-on-surface shrink-0 font-medium tabular-nums">
        {m.proxies_group_available({ available, total })}
      </span>

      {chainText && (
        <span className="min-w-0 truncate" title={chainText}>
          {chainText}
        </span>
      )}
    </span>
  )
}

/** Live speed of the traffic routed through the group's current node. */
export function GroupTrafficSpeed({
  groupName,
  only,
}: {
  groupName: string
  /** Keep just the busier direction, for when the header has little room. */
  only?: 'fastest'
}) {
  const speed = useGroupTrafficSpeed(groupName)

  const rates = [
    { direction: 'down', Icon: ArrowDownwardAltRounded, value: speed.download },
    { direction: 'up', Icon: ArrowUpwardAltRounded, value: speed.upload },
  ]

  const shown =
    only === 'fastest'
      ? [rates.reduce((max, rate) => (rate.value > max.value ? rate : max))]
      : rates

  return (
    <span className="flex shrink-0 items-center gap-2 text-xs">
      {shown.map(({ direction, Icon, value }) => (
        <span key={direction} className="flex items-center gap-1">
          <Icon className="text-on-surface-variant size-3.5 shrink-0" />

          <span className="text-on-surface tabular-nums">
            {filesize(value, { standard: 'iec' })}/s
          </span>
        </span>
      ))}
    </span>
  )
}

export function GroupMeta({
  name,
  status,
  speed,
  speedCompact,
  rotateMs = DEFAULT_ROTATE_MS,
}: {
  name: string
  status: ReactNode
  speed: ReactNode
  speedCompact: ReactNode
  /** How long each side of the collapsed slot stays visible. */
  rotateMs?: number
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const measureRef = useRef<HTMLDivElement>(null)

  const [compact, setCompact] = useState(false)
  const [showSpeed, setShowSpeed] = useState(true)

  // The header keeps the name beside the status and the speed while they fit.
  // Once the row would overflow, the two share one slot and take turns. The
  // reserved speed width keeps the decision stable while the live value moves.
  useEffect(() => {
    const container = containerRef.current
    const measure = measureRef.current

    if (!container || !measure) {
      return
    }

    const check = () => setCompact(measure.scrollWidth > container.clientWidth)

    check()

    const observer = new ResizeObserver(check)
    observer.observe(container)

    return () => observer.disconnect()
  }, [name, status])

  // Rotate only while collapsed; a click flips at once and restarts nothing.
  useEffect(() => {
    if (!compact) {
      setShowSpeed(true)
      return
    }

    const interval = setInterval(
      () => setShowSpeed((shown) => !shown),
      rotateMs,
    )

    return () => clearInterval(interval)
  }, [compact, rotateMs])

  return (
    <div
      ref={containerRef}
      className="relative min-w-0"
      data-slot="proxies-group-meta"
    >
      <div
        aria-hidden
        className="pointer-events-none invisible absolute inset-0 overflow-hidden"
      >
        <div
          ref={measureRef}
          className="flex w-max items-center gap-2 whitespace-nowrap"
        >
          <span className="font-medium">{name}</span>
          {status}
          {/* Reserved speed width so live values do not flip the layout. */}
          <span className="w-44 shrink-0" />
        </div>
      </div>

      <div className="flex min-w-0 items-center gap-2 whitespace-nowrap">
        <span className="min-w-0 truncate font-medium" title={name}>
          {name}
        </span>

        {compact ? (
          <button
            type="button"
            data-showing={showSpeed ? 'speed' : 'status'}
            className="focus-visible:outline-primary flex min-w-0 cursor-pointer items-center rounded outline-none focus-visible:outline-2"
            onClick={() => setShowSpeed((shown) => !shown)}
          >
            <ActionSwap
              contentKey={showSpeed ? 'speed' : 'status'}
              className="items-center gap-2"
            >
              {showSpeed ? speedCompact : status}
            </ActionSwap>
          </button>
        ) : (
          <>
            {status}
            {speed}
          </>
        )}
      </div>
    </div>
  )
}
