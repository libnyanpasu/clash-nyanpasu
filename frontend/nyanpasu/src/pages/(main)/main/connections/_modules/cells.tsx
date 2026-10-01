import dayjs from 'dayjs'
import { createContext, Fragment, useContext } from 'react'
import HighlightText from '@/components/ui/highlight-text'
import parseTraffic from '@/utils/parse-traffic'
import { cn } from '@nyanpasu/utils'

// Values the core or the traffic history use when a field is unknown.
const isPlaceholder = (value: string) => !value || value === 'unknown'

export function TextCell({
  value,
  search,
  primary,
}: {
  value: string
  search: string
  primary?: boolean
}) {
  if (isPlaceholder(value)) {
    return <span className="text-on-surface-variant/50">{value || '—'}</span>
  }

  return (
    <HighlightText
      searchText={search}
      className={cn(primary && 'text-on-surface font-medium')}
    >
      {value}
    </HighlightText>
  )
}

/** `chains` in wire order: the exit first, the outermost group last. */
export function ChainCell({
  chains,
  search,
}: {
  chains: readonly string[]
  search: string
}) {
  const path = [...chains].reverse()

  return (
    <span>
      {path.map((name, index) => (
        <Fragment key={index}>
          {index > 0 && <span className="text-outline mx-1">›</span>}

          <HighlightText
            searchText={search}
            className={cn(index === path.length - 1 && 'text-on-surface')}
          >
            {name}
          </HighlightText>
        </Fragment>
      ))}
    </span>
  )
}

export function RuleCell({
  kind,
  payload,
  search,
}: {
  kind: string
  payload: string
  search: string
}) {
  if (!payload) {
    return <HighlightText searchText={search}>{kind}</HighlightText>
  }

  return (
    <span>
      <HighlightText searchText={search} className="text-on-surface">
        {payload}
      </HighlightText>

      <HighlightText searchText={search} className="text-outline ml-1.5">
        {kind}
      </HighlightText>
    </span>
  )
}

/** Bytes, or bytes per second with `rate`; zero is dimmed. */
export function TrafficCell({
  value,
  rate,
}: {
  value: number
  rate?: boolean
}) {
  return (
    <span
      className={cn(
        value > 0 ? 'text-on-surface' : 'text-on-surface-variant/50',
      )}
    >
      {parseTraffic(value).join(' ')}
      {rate && '/s'}
    </span>
  )
}

/**
 * The table's current data. Rows skip re-rendering while their values stay
 * the same, so relative times subscribe to it to move on with every sample.
 */
export const RowsTickContext = createContext<unknown>(null)

/** A time as "x ago", with the exact time on hover. */
export function RelativeTimeCell({ ms }: { ms: number }) {
  useContext(RowsTickContext)

  const time = dayjs(ms)

  return (
    <span title={time.format('YYYY-MM-DD HH:mm:ss')}>{time.fromNow()}</span>
  )
}
