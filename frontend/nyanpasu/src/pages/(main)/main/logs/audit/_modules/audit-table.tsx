import { cva } from 'class-variance-authority'
import { memo, useEffect } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { useScrollAreaViewportElement } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import type { AuditRow } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useDataPageBottomInset } from '../../../_modules/data-page'
import {
  AUDIT_TRANSPORTS,
  transportLabel,
  type AuditTransport,
} from './audit-filter-sidebar'

// Rows hold one line each, so the virtual list never measures them.
const ROW_HEIGHT = 40

// Outcomes of calls that did not complete normally.
const FAILED_OUTCOMES = new Set(['failure', 'panicked', 'failed'])

const auditCellVariants = cva([
  'truncate px-3 first:pl-4 last:pr-4',
  'border-outline-variant/40 border-b',
])

const auditHeaderCellVariants = cva([
  'bg-mixed-background sticky top-0 z-10 h-10 truncate px-3 text-left',
  'text-on-surface-variant text-xs font-medium',
  'border-outline-variant/25 border-b first:pl-4 last:pr-4',
])

const auditRowKey = (row: AuditRow) =>
  `${row.at}:${row.owner}:${row.invocation_id}:${row.phase}:${row.outcome}`

const isKnownTransport = (transport: string): transport is AuditTransport =>
  (AUDIT_TRANSPORTS as readonly string[]).includes(transport)

const AuditTableRow = memo(function AuditTableRow({ row }: { row: AuditRow }) {
  const time = new Date(row.at).toLocaleString()
  const result = row.outcome ? `${row.phase} · ${row.outcome}` : row.phase
  const failed =
    row.phase === 'rejected' ||
    (row.outcome != null && FAILED_OUTCOMES.has(row.outcome))
  const code = [row.kind, row.code].filter(Boolean).join(' · ')

  return (
    <tr
      className="hover:bg-on-surface/4 text-sm"
      data-slot="logs-audit-row"
      style={{ height: ROW_HEIGHT }}
    >
      <td className={cn(auditCellVariants(), 'font-mono text-xs')}>
        <time title={time}>{time}</time>
      </td>
      <td className={auditCellVariants()}>
        {isKnownTransport(row.transport)
          ? transportLabel(row.transport)
          : row.transport}
      </td>
      <td
        className={cn(auditCellVariants(), 'font-mono text-xs')}
        title={row.method}
      >
        {row.method}
      </td>
      <td
        className={cn(auditCellVariants(), 'font-mono text-xs')}
        title={row.owner}
      >
        {row.owner}
      </td>
      <td
        className={cn(
          auditCellVariants(),
          'font-mono text-xs',
          failed && 'text-error',
        )}
        title={result}
      >
        {result}
      </td>
      <td className={cn(auditCellVariants(), 'text-right tabular-nums')}>
        {m.logs_audit_duration_ms({
          duration: (row.duration_ns / 1_000_000).toFixed(1),
        })}
      </td>
      <td
        className={cn(auditCellVariants(), 'font-mono text-xs')}
        title={code || undefined}
      >
        {code || '—'}
      </td>
    </tr>
  )
})

/**
 * The loaded audit records, newest first, with a button that loads older
 * ones. Place inside a `ScrollArea` that scrolls both ways.
 */
export function AuditTable({
  rows,
  hasMore,
  loadingMore,
  onLoadMore,
}: {
  rows: AuditRow[]
  hasMore: boolean
  loadingMore: boolean
  onLoadMore: () => void
}) {
  const viewportElement = useScrollAreaViewportElement()
  const bottomInset = useDataPageBottomInset()
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewportElement,
    getItemKey: (index) => auditRowKey(rows[index]),
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
    useFlushSync: false,
  })
  const items = virtualizer.getVirtualItems()
  const topSpacer = items[0]?.start ?? 0
  const bottomSpacer = Math.max(
    0,
    virtualizer.getTotalSize() - (items.at(-1)?.end ?? 0),
  )

  useEffect(() => {
    // Reconnect after ScrollArea has attached its viewport ref.
    virtualizer.measure()
  }, [virtualizer])

  return (
    <div data-slot="logs-audit-table" style={{ paddingBottom: bottomInset }}>
      <table className="w-full min-w-5xl table-fixed border-separate border-spacing-0 select-text">
        <colgroup>
          <col className="w-44" />
          <col className="w-20" />
          <col />
          <col className="w-36" />
          <col className="w-44" />
          <col className="w-24" />
          <col className="w-48" />
        </colgroup>
        <thead>
          <tr>
            <th className={auditHeaderCellVariants()}>
              {m.logs_audit_column_time()}
            </th>
            <th className={auditHeaderCellVariants()}>
              {m.logs_audit_column_transport()}
            </th>
            <th className={auditHeaderCellVariants()}>
              {m.logs_audit_column_method()}
            </th>
            <th className={auditHeaderCellVariants()}>
              {m.logs_audit_column_caller()}
            </th>
            <th className={auditHeaderCellVariants()}>
              {m.logs_audit_column_result()}
            </th>
            <th className={cn(auditHeaderCellVariants(), 'text-right')}>
              {m.logs_audit_column_duration()}
            </th>
            <th className={auditHeaderCellVariants()}>
              {m.logs_audit_column_code()}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr aria-hidden style={{ height: topSpacer }} />
          {items.map((item) => (
            <AuditTableRow key={item.key} row={rows[item.index]} />
          ))}
          <tr aria-hidden style={{ height: bottomSpacer }} />
        </tbody>
      </table>

      {!rows.length && !hasMore && (
        <EmptyState
          className="py-16"
          data-slot="logs-no-logs"
          message={m.logs_empty_message()}
        />
      )}

      {hasMore && (
        <div className="flex justify-center p-4" data-slot="logs-audit-more">
          <Button
            variant="flat"
            loading={loadingMore}
            disabled={loadingMore}
            onClick={onLoadMore}
          >
            {m.logs_load_older()}
          </Button>
        </div>
      )}
    </div>
  )
}
