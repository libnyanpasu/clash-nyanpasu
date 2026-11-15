import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import WarningRounded from '~icons/material-symbols/warning-rounded'
import { useMemo } from 'react'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { useRpcAuditLog } from '@nyanpasu/query'
import type { AuditFilter } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { createFileRoute } from '@tanstack/react-router'
import { logPanelVariants, useSlowLoading } from '../_modules/log-viewer'
import { LogsDataPage } from '../_modules/logs-layout'
import { dataPageLayoutVariants } from '../../_modules/data-page'
import {
  AUDIT_OUTCOMES,
  AUDIT_TRANSPORTS,
  AuditFilterSidebar,
} from './_modules/audit-filter-sidebar'
import { AuditTable } from './_modules/audit-table'

export const Route = createFileRoute('/(main)/main/logs/audit/')({
  component: RouteComponent,
  validateSearch: z.object({
    transport: z.enum(AUDIT_TRANSPORTS).optional(),
    outcome: z.enum(AUDIT_OUTCOMES).optional(),
  }),
})

function RouteComponent() {
  const { transport, outcome } = Route.useSearch()

  const filter = useMemo<AuditFilter>(
    () => ({
      transport: transport ?? null,
      // A rejected call has no outcome; its phase selects it.
      phase: outcome === 'rejected' ? 'rejected' : null,
      outcome: outcome === 'rejected' ? null : (outcome ?? null),
    }),
    [transport, outcome],
  )
  const { query, refresh } = useRpcAuditLog(filter)

  const rows = useMemo(
    () => query.data?.pages.flatMap((page) => page.rows) ?? [],
    [query.data],
  )
  // The count only grows within a run; the latest page read has the newest.
  const dropped = Math.max(
    0,
    ...(query.data?.pages ?? []).map((page) => page.dropped),
  )
  const slowLoading = useSlowLoading(query.isPending)

  return (
    <div className={dataPageLayoutVariants()} data-slot="logs-audit-page">
      <AuditFilterSidebar transport={transport} outcome={outcome} />

      <LogsDataPage
        source="audit"
        actions={
          <Button
            variant="glass"
            icon
            disabled={query.isFetching}
            onClick={() => refresh()}
            aria-label={m.logs_audit_refresh()}
            title={m.logs_audit_refresh()}
            className="focus-visible:ring-primary shrink-0 focus-visible:ring-2"
          >
            <RefreshRounded aria-hidden className="size-5" />
          </Button>
        }
      >
        <div className={logPanelVariants()} data-slot="logs-audit">
          {dropped > 0 && (
            <div
              className={cn(
                'flex items-center gap-2 px-4 py-2 text-sm',
                'bg-tertiary-container text-on-tertiary-container',
              )}
              role="alert"
              data-slot="logs-audit-dropped"
            >
              <WarningRounded aria-hidden className="size-5 shrink-0" />
              <span>{m.logs_audit_dropped({ count: dropped })}</span>
            </div>
          )}

          <div
            className="text-on-surface-variant flex flex-wrap items-center gap-2 px-4 py-2 text-xs"
            role="status"
            data-slot="logs-audit-status"
          >
            <span>{m.logs_audit_scope_hint()}</span>
            {slowLoading && <span>{m.logs_loading()}</span>}
            {query.isError && (
              <>
                <span className="text-error">{m.logs_audit_unavailable()}</span>
                <Button onClick={() => refresh()}>{m.logs_retry()}</Button>
              </>
            )}
          </div>

          <ScrollArea
            key={`${transport}:${outcome}`}
            className="min-h-0 flex-1"
            scrollbars="both"
          >
            {query.data && (
              <AuditTable
                rows={rows}
                hasMore={query.hasNextPage}
                loadingMore={query.isFetchingNextPage}
                onLoadMore={() => query.fetchNextPage()}
              />
            )}
          </ScrollArea>
        </div>
      </LogsDataPage>
    </div>
  )
}
