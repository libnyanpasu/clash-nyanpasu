import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import ChevronLeftRounded from '~icons/material-symbols/chevron-left-rounded'
import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import SwapVertRounded from '~icons/material-symbols/swap-vert-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card } from '@nyanpasu/ui/card'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { GlassControlGroup } from '@nyanpasu/ui/glass-control-group'
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
import { getLocale } from '@/paraglide/runtime'
import parseTraffic from '@/utils/parse-traffic'
import {
  useMeowStatistics,
  useMeowTrafficLedger,
  type MeowCore,
} from '@nyanpasu/query'
import type { MeowTrafficGroup } from '@nyanpasu/rpc/types'
import Notice from '../../_modules/notice'
import FilterChip from '../../../_modules/filter-chip'

const periods = {
  today: m.meow_traffic_today,
  month: m.meow_traffic_this_month,
  previous: m.meow_traffic_last_month,
}
type Period = keyof typeof periods

const groups = {
  host: m.traffic_dimension_target,
  app: m.meow_traffic_app,
  policy: m.meow_traffic_policy,
  interface: m.meow_traffic_interface,
} satisfies Record<MeowTrafficGroup, () => string>

const groupKeys = Object.keys(groups) as MeowTrafficGroup[]
const periodKeys = Object.keys(periods) as Period[]

type Filter = { group: MeowTrafficGroup; name: string }
const ROWS_PER_PAGE = 50

type Usage = {
  name: string
  upload: number
  download: number
  requests?: number
  via_upload?: number
  via_download?: number
}

const nameOf = (name: string, group: MeowTrafficGroup) =>
  name === '*' && group === 'host'
    ? m.meow_traffic_other_ips()
    : name || m.topology_unknown()

export default function MeowTraffic({ core }: { core: MeowCore }) {
  const [view, setView] = useState<'ledger' | 'statistics'>('ledger')

  return (
    <div className="flex min-w-0 flex-col gap-4" data-slot="meow-traffic">
      <GlassControlGroup className="w-fit max-w-full">
        <SegmentedButton
          variant="glass"
          size="sm"
          value={view}
          aria-label={m.traffic_sidebar()}
          onValueChange={(next) => {
            if (next === 'ledger' || next === 'statistics') setView(next)
          }}
        >
          <SegmentedButtonItem value="ledger">
            {m.meow_traffic_ledger()}
          </SegmentedButtonItem>
          <SegmentedButtonItem value="statistics">
            {m.meow_traffic_statistics()}
          </SegmentedButtonItem>
        </SegmentedButton>
      </GlassControlGroup>
      {view === 'ledger' ? <Ledger core={core} /> : <Statistics core={core} />}
    </div>
  )
}

/** The `YYYY-MM-DD` day Meow records, in the device's time zone. */
const localDay = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`

function Ledger({ core }: { core: MeowCore }) {
  const [period, setPeriod] = useState<Period>('today')
  const [group, setGroup] = useState<MeowTrafficGroup>('host')
  const [filters, setFilters] = useState<Filter[]>([])
  const choices = groupKeys.filter(
    (key) => !filters.some((filter) => filter.group === key),
  )
  // Standalone Meow records days in the device's time zone.
  const today = new Date()
  const year = today.getFullYear()
  const month = today.getMonth()
  const from =
    period === 'today'
      ? today
      : new Date(year, period === 'month' ? month : month - 1, 1)
  const to = period === 'previous' ? new Date(year, month, 0) : today
  const fromDay = localDay(from)
  const toDay = localDay(to)
  const query = useMeowTrafficLedger(core, {
    from: fromDay,
    to: toDay,
    by: group,
    host: null,
    app: null,
    policy: null,
    interface: null,
    ...Object.fromEntries(filters.map(({ group, name }) => [group, name])),
  })
  const report = query.data
  const nextGroup = choices.find((key) => key !== group)
  const dateFormat = new Intl.DateTimeFormat(getLocale(), {
    dateStyle: 'medium',
  })

  return (
    <>
      <div className="flex flex-wrap items-center gap-2">
        <Select
          variant="chip"
          value={period}
          onValueChange={(next) => {
            if (periodKeys.includes(next as Period)) setPeriod(next as Period)
          }}
        >
          <SelectTrigger aria-label={m.traffic_range_label()}>
            <SelectValue>{periods[period]()}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            {periodKeys.map((key) => (
              <SelectItem key={key} value={key}>
                {periods[key]()}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          variant="chip"
          value={group}
          onValueChange={(next) => {
            if (choices.includes(next as MeowTrafficGroup))
              setGroup(next as MeowTrafficGroup)
          }}
        >
          <SelectTrigger aria-label={m.meow_traffic_group()}>
            <SelectValue>{groups[group]()}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            {choices.map((key) => (
              <SelectItem key={key} value={key}>
                {groups[key]()}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-on-surface-variant text-xs tabular-nums">
          {period === 'today'
            ? dateFormat.format(today)
            : dateFormat.formatRange(from, to)}
        </span>
        <Button
          type="button"
          className="ml-auto size-8"
          icon
          aria-label={m.inspect_refresh()}
          disabled={query.isFetching}
          onClick={() => query.refetch()}
        >
          <RefreshRounded className="size-5" aria-hidden />
        </Button>
      </div>
      {filters.length > 0 && (
        <div
          className="flex flex-wrap items-center gap-2"
          aria-label={m.traffic_filters_label()}
        >
          {filters.map((filter) => {
            const label = `${groups[filter.group]()} · ${nameOf(filter.name, filter.group)}`
            return (
              <FilterChip
                key={filter.group}
                label={label}
                title={label}
                onRemove={() => {
                  setFilters((previous) =>
                    previous.filter(({ group }) => group !== filter.group),
                  )
                  setGroup(filter.group)
                }}
              />
            )
          })}
        </div>
      )}
      {query.isPending && <Notice>{m.logs_loading()}</Notice>}
      {query.isError && <Notice error>{m.meow_traffic_read_failed()}</Notice>}
      {report === null && (
        <Notice>{m.meow_traffic_ledger_unsupported()}</Notice>
      )}
      {report?.enabled === false && (
        <Notice>{m.meow_traffic_ledger_disabled()}</Notice>
      )}
      {report?.enabled && (
        <>
          <TrafficTotals upload={report.upload} download={report.download} />
          <TrafficRows
            key={JSON.stringify([fromDay, toDay, group, filters])}
            group={group}
            title={groups[group]()}
            detail={m.meow_traffic_requests({
              count: report.requests.toLocaleString(getLocale()),
            })}
            rows={report.rows}
            onOpen={
              nextGroup &&
              ((name) => {
                setFilters((previous) => [...previous, { group, name }])
                setGroup(nextGroup)
              })
            }
          />
        </>
      )}
    </>
  )
}

function Statistics({ core }: { core: MeowCore }) {
  const query = useMeowStatistics(core)
  const statistics = query.data

  return (
    <>
      <p className="text-on-surface-variant m-0! text-sm">
        {m.meow_traffic_since_start()}
      </p>
      {query.isPending && <Notice>{m.logs_loading()}</Notice>}
      {query.isError && <Notice error>{m.meow_traffic_read_failed()}</Notice>}
      {statistics === null && (
        <Notice>{m.meow_traffic_statistics_unsupported()}</Notice>
      )}
      {statistics && (
        <>
          <TrafficTotals
            upload={statistics.uploadTotal}
            download={statistics.downloadTotal}
          />
          <TrafficRows
            title={m.meow_traffic_policy()}
            detail={m.meow_traffic_policy_hint()}
            rows={Object.entries(statistics.proxies)
              .filter(([, usage]) => usage.upload > 0 || usage.download > 0)
              .sort(([left], [right]) =>
                left < right ? -1 : left > right ? 1 : 0,
              )
              .map(([name, usage]) => ({ name, ...usage }))}
          />
        </>
      )}
    </>
  )
}

function TrafficTotals({
  upload,
  download,
}: {
  upload: number
  download: number
}) {
  const cards = [
    {
      label: m.traffic_stat_total(),
      bytes: upload + download,
      icon: SwapVertRounded,
    },
    {
      label: m.connections_field_download(),
      bytes: download,
      icon: ArrowDownwardRounded,
    },
    {
      label: m.connections_field_upload(),
      bytes: upload,
      icon: ArrowUpwardRounded,
    },
  ]

  return (
    <dl
      className="grid grid-cols-1 gap-2.5 sm:grid-cols-3"
      data-slot="meow-traffic-totals"
    >
      {cards.map(({ label, bytes, icon: Icon }) => (
        <Card
          key={label}
          variant="outline"
          className="flex items-center gap-3 px-4 py-3.5"
        >
          <span className="bg-secondary-container text-on-secondary-container rounded-large grid size-9 shrink-0 place-items-center">
            <Icon className="size-5" aria-hidden />
          </span>
          <div>
            <dt className="text-on-surface-variant text-xs">{label}</dt>
            <dd className="text-xl font-medium tabular-nums">
              {parseTraffic(bytes).join(' ')}
            </dd>
          </div>
        </Card>
      ))}
    </dl>
  )
}

function TrafficRows({
  group,
  title,
  detail,
  rows,
  onOpen,
}: {
  group?: MeowTrafficGroup
  title: string
  detail: string
  rows: Usage[]
  onOpen?: (name: string) => void
}) {
  const [page, setPage] = useState(0)
  const pageCount = Math.ceil(rows.length / ROWS_PER_PAGE)
  const shownPage = Math.min(page, Math.max(0, pageCount - 1))
  const start = shownPage * ROWS_PER_PAGE
  if (rows.length === 0) return <EmptyState message={m.traffic_empty()} />

  return (
    <Card variant="outline" className="min-w-0">
      <div className="border-outline-variant/60 flex flex-wrap items-baseline justify-between gap-2 border-b px-4 py-3">
        <h2 className="m-0! text-base! font-medium">{title}</h2>
        <p className="text-on-surface-variant m-0! text-xs">{detail}</p>
        {pageCount > 1 && (
          <div className="ml-auto flex items-center gap-1 text-xs tabular-nums">
            <Button
              type="button"
              icon
              className="size-7"
              aria-label={m.meow_traffic_previous_page()}
              disabled={shownPage === 0}
              onClick={() => setPage(shownPage - 1)}
            >
              <ChevronLeftRounded className="size-4" aria-hidden />
            </Button>
            <span>
              {m.meow_traffic_page({
                page: (shownPage + 1).toLocaleString(getLocale()),
                count: pageCount.toLocaleString(getLocale()),
              })}
            </span>
            <Button
              type="button"
              icon
              className="size-7"
              aria-label={m.meow_traffic_next_page()}
              disabled={shownPage === pageCount - 1}
              onClick={() => setPage(shownPage + 1)}
            >
              <ChevronRightRounded className="size-4" aria-hidden />
            </Button>
          </div>
        )}
      </div>
      <div className="overflow-x-auto">
        <table aria-label={title} className="w-full border-collapse text-sm">
          <thead className="bg-surface-container/60 text-on-surface-variant text-xs">
            <tr>
              <th scope="col" className="px-4 py-2.5 text-left font-medium">
                {m.meow_traffic_name()}
              </th>
              <th scope="col" className="px-3 py-2.5 text-right font-medium">
                {m.connections_field_download()}
              </th>
              <th scope="col" className="px-3 py-2.5 text-right font-medium">
                {m.connections_field_upload()}
              </th>
              <th scope="col" className="px-4 py-2.5 text-right font-medium">
                {m.traffic_stat_total()}
              </th>
            </tr>
          </thead>
          <tbody className="divide-outline-variant/45 divide-y">
            {rows
              .slice(start, start + ROWS_PER_PAGE)
              .map(
                ({
                  name,
                  upload,
                  download,
                  requests,
                  via_upload,
                  via_download,
                }) => {
                  const label = group ? nameOf(name, group) : name
                  const via = (via_upload ?? 0) > 0 || (via_download ?? 0) > 0
                  return (
                    <tr key={name}>
                      <th
                        scope="row"
                        className="max-w-72 px-4 py-2.5 text-left font-medium"
                      >
                        {onOpen ? (
                          <button
                            type="button"
                            className="text-primary focus-visible:ring-primary rounded-extra-small inline-flex max-w-full items-center gap-1 text-start hover:underline focus-visible:ring-2"
                            onClick={() => onOpen(name)}
                          >
                            <span className="truncate" title={label}>
                              {label}
                            </span>
                            <ChevronRightRounded
                              className="size-4 shrink-0"
                              aria-hidden
                            />
                          </button>
                        ) : (
                          <span className="block truncate" title={label}>
                            {label}
                          </span>
                        )}
                        {requests !== undefined && (
                          <span className="text-on-surface-variant block text-xs font-normal">
                            {m.meow_traffic_requests({
                              count: requests.toLocaleString(getLocale()),
                            })}
                            {via && ` · ${m.meow_traffic_via_hop()}`}
                          </span>
                        )}
                      </th>
                      <td className="px-3 text-right whitespace-nowrap tabular-nums">
                        {parseTraffic(download).join(' ')}
                      </td>
                      <td className="px-3 text-right whitespace-nowrap tabular-nums">
                        {parseTraffic(upload).join(' ')}
                      </td>
                      <td className="px-4 text-right font-medium whitespace-nowrap tabular-nums">
                        {parseTraffic(upload + download).join(' ')}
                      </td>
                    </tr>
                  )
                },
              )}
          </tbody>
        </table>
      </div>
    </Card>
  )
}
