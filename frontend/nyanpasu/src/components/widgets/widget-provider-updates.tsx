import CloudSyncRounded from '~icons/material-symbols/cloud-sync-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { getLocale } from '@/paraglide/runtime'
import { formatRelativeTime } from '@/utils/date'
import {
  MutationUnconfirmedError,
  useClashProxiesProvider,
  useClashRulesProvider,
  useUpdateClashProxiesProvider,
  useUpdateClashRulesProvider,
} from '@nyanpasu/query'
import { useMutationState } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { useWidgetHeight } from './use-widget-height'
import { WidgetId } from './widget-config'
import WidgetItem from './widget-item'
import { WidgetTitle } from './widget-ui'

const PROVIDER_ROW_HEIGHT = 44

type ProviderKind = 'proxy' | 'rule'
type ProviderRow = {
  kind: ProviderKind
  name: string
  count: number | null
  type: string | null
  vehicleType: string | null
  updatedAt: string | null | undefined
}
type OperationFeedback = Record<string, 'failed' | 'unconfirmed'>

const providerKey = (kind: ProviderKind, name: string) => `${kind}:${name}`

function ProviderUpdatesPreview({ id, onCloseClick }: WidgetComponentProps) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProviderUpdates}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-provider-updates-card">
        <CardContent className="flex size-full flex-col gap-3">
          <WidgetTitle icon={CloudSyncRounded}>
            {m.dashboard_widget_provider_updates_title()}
          </WidgetTitle>
          <p className="text-on-surface-variant text-sm">
            {m.dashboard_widget_provider_updates_preview()}
          </p>
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

function ProviderUpdatesLive({
  id,
  onCloseClick,
  canAct,
}: WidgetComponentProps & { canAct: boolean }) {
  const config = useWidgetConfig(id, WidgetId.ProviderUpdates)
  const allowedKinds =
    config.kinds === 'both'
      ? (['proxy', 'rule'] as const)
      : ([config.kinds] as const)
  const proxies = useClashProxiesProvider({
    enabled: allowedKinds.includes('proxy'),
  })
  const rules = useClashRulesProvider({
    enabled: allowedKinds.includes('rule'),
  })
  const updateProxy = useUpdateClashProxiesProvider()
  const updateRule = useUpdateClashRulesProvider()
  const [feedback, setFeedback] = useState<OperationFeedback>({})
  const { ref: listRef, height: listHeight } = useWidgetHeight<HTMLDivElement>()
  const clearFeedback = (key: string) =>
    setFeedback((previous) => {
      const next = { ...previous }
      delete next[key]
      return next
    })
  const pendingProxyNames = useMutationState({
    filters: { mutationKey: ['updateProxyProvider'], status: 'pending' },
    select: (mutation) => mutation.state.variables as string,
  })
  const pendingRuleNames = useMutationState({
    filters: {
      mutationKey: ['clashApiUpdateProvidersRules'],
      status: 'pending',
    },
    select: (mutation) => mutation.state.variables as string,
  })

  const rows: ProviderRow[] = [
    ...Object.values(proxies.data ?? {}).map((provider) => ({
      kind: 'proxy' as const,
      name: provider.name,
      count: provider.proxyCount,
      type: provider.type,
      vehicleType: provider.vehicleType,
      updatedAt: provider.updatedAt,
    })),
    ...Object.values(rules.data ?? {}).map((provider) => ({
      kind: 'rule' as const,
      name: provider.name,
      count: provider.ruleCount,
      type: provider.type,
      vehicleType: provider.vehicleType,
      updatedAt: provider.updatedAt,
    })),
  ].sort(
    (left, right) =>
      left.kind.localeCompare(right.kind) ||
      left.name.localeCompare(right.name),
  )

  const rowsByKey = new Map(
    rows
      .filter((row) => allowedKinds.includes(row.kind))
      .map((row) => [providerKey(row.kind, row.name), row]),
  )
  const selectedRows = config.resources.length
    ? config.resources
        .filter((resource) => allowedKinds.includes(resource.kind))
        .slice(0, config.maxItems)
        .map((resource) => ({
          kind: resource.kind,
          name: resource.name,
          row: rowsByKey.get(providerKey(resource.kind, resource.name)),
        }))
    : rows
        .filter((row) => allowedKinds.includes(row.kind))
        .slice(0, config.maxItems)
        .map((row) => ({ kind: row.kind, name: row.name, row }))
  const visibleCount =
    listHeight == null
      ? 1
      : Math.max(1, Math.floor((listHeight + 4) / PROVIDER_ROW_HEIGHT))

  const proxyCount =
    allowedKinds.includes('proxy') && proxies.data !== undefined
      ? String(Object.keys(proxies.data).length)
      : '—'
  const ruleCount =
    allowedKinds.includes('rule') && rules.data !== undefined
      ? String(Object.keys(rules.data).length)
      : '—'
  const isLoading =
    (allowedKinds.includes('proxy') && proxies.isLoading) ||
    (allowedKinds.includes('rule') && rules.isLoading)
  const hasError =
    (allowedKinds.includes('proxy') && proxies.isError) ||
    (allowedKinds.includes('rule') && rules.isError)

  const update = async (kind: ProviderKind, name: string) => {
    const key = providerKey(kind, name)
    if (
      feedback[key] === 'unconfirmed' ||
      (kind === 'proxy' ? pendingProxyNames : pendingRuleNames).includes(name)
    )
      return
    clearFeedback(key)
    try {
      if (kind === 'proxy') await updateProxy.mutateAsync(name)
      else await updateRule.mutateAsync(name)
    } catch (error) {
      if (kind === 'proxy') await proxies.refetch()
      else await rules.refetch()
      setFeedback((previous) => ({
        ...previous,
        [key]:
          error instanceof MutationUnconfirmedError ? 'unconfirmed' : 'failed',
      }))
    }
  }

  const handleUpdate = (kind: ProviderKind, name: string) => {
    update(kind, name).catch(() => undefined)
  }

  const handleCheck = async (kind: ProviderKind, name: string) => {
    const result =
      kind === 'proxy' ? await proxies.refetch() : await rules.refetch()
    if (!result.isError) clearFeedback(providerKey(kind, name))
  }

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProviderUpdates}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-provider-updates-card">
        <CardContent className="flex size-full min-h-0 flex-col gap-2">
          <div className="flex items-center justify-between gap-2">
            <WidgetTitle className="flex-1" icon={CloudSyncRounded}>
              {m.dashboard_widget_provider_updates_title()}
            </WidgetTitle>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="basic"
                  className="size-7 shrink-0"
                  icon
                  aria-label={m.dashboard_widget_provider_updates_details()}
                  asChild
                >
                  <Link
                    aria-disabled={!canAct}
                    tabIndex={canAct ? 0 : -1}
                    className={!canAct ? 'pointer-events-none opacity-50' : ''}
                    onClick={(event) => {
                      if (!canAct) event.preventDefault()
                    }}
                    to="/main/providers"
                  >
                    <OpenInNewRounded className="size-4" />
                  </Link>
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                {m.dashboard_widget_provider_updates_details()}
              </TooltipContent>
            </Tooltip>
          </div>

          <p className="text-on-surface-variant text-xs">
            {m.dashboard_widget_provider_updates_counts({
              proxies: proxyCount,
              rules: ruleCount,
            })}
          </p>

          {hasError && (
            <p className="text-error text-xs" role="status">
              {m.dashboard_widget_provider_updates_read_failed()}
            </p>
          )}

          {Object.entries(feedback).map(([key, state]) => {
            const separator = key.indexOf(':')
            const name = key.slice(separator + 1)
            const message =
              state === 'unconfirmed'
                ? m.dashboard_widget_provider_updates_unconfirmed()
                : m.dashboard_widget_provider_updates_failed()
            return (
              <p
                className="text-error text-xs break-words"
                key={key}
                role="status"
              >
                {name}: {message}
              </p>
            )
          })}

          {isLoading && rows.length === 0 ? (
            <p className="text-on-surface-variant text-sm" role="status">
              {m.dashboard_widget_provider_updates_loading()}
            </p>
          ) : selectedRows.length === 0 ? (
            <p className="text-on-surface-variant flex-1 text-sm" role="status">
              {hasError
                ? m.dashboard_widget_provider_updates_read_failed()
                : m.dashboard_widget_provider_updates_empty()}
            </p>
          ) : (
            <div
              ref={listRef}
              className="min-h-0 flex-1"
              data-slot="widget-provider-updates-list"
            >
              <ul className="flex flex-col gap-1">
                {selectedRows
                  .slice(0, visibleCount)
                  .map(({ kind, name, row }) => {
                    const key = providerKey(kind, name)
                    const hasKindData =
                      kind === 'proxy'
                        ? proxies.data !== undefined
                        : rules.data !== undefined
                    const missing = !row && hasKindData
                    const pending =
                      kind === 'proxy'
                        ? pendingProxyNames.includes(name)
                        : pendingRuleNames.includes(name)
                    const kindHasError =
                      kind === 'proxy' ? proxies.isError : rules.isError
                    const rowFeedback = feedback[key] ?? null
                    const to =
                      kind === 'proxy'
                        ? '/main/providers/proxies/$key'
                        : '/main/providers/rules/$key'

                    return (
                      <li
                        className="hover:bg-surface-variant/40 flex min-h-10 min-w-0 shrink-0 items-center gap-2 rounded-xl px-2 py-1 text-xs"
                        key={key}
                      >
                        <div className="min-w-0 flex-1">
                          {missing ? (
                            <div className="truncate font-medium">{name}</div>
                          ) : (
                            <Link
                              aria-disabled={!canAct}
                              tabIndex={canAct ? 0 : -1}
                              onClick={(event) => {
                                if (!canAct) event.preventDefault()
                              }}
                              className={`block truncate font-medium hover:underline ${!canAct ? 'pointer-events-none opacity-50' : ''}`}
                              to={to}
                              params={{ key: name }}
                            >
                              {name}
                            </Link>
                          )}
                          <div className="text-on-surface-variant truncate">
                            {row
                              ? `${row.vehicleType ?? '—'} · ${row.type ?? '—'} · ${row.count ?? '—'} · ${formatRelativeTime(row.updatedAt, Date.now(), getLocale())}`
                              : missing
                                ? m.dashboard_widget_provider_updates_missing()
                                : isLoading
                                  ? m.dashboard_widget_provider_updates_loading()
                                  : m.dashboard_widget_provider_updates_read_failed()}
                          </div>
                        </div>
                        {row && (
                          <Button
                            className="size-8 shrink-0"
                            icon
                            loading={pending}
                            disabled={
                              !canAct ||
                              pending ||
                              (rowFeedback === 'unconfirmed'
                                ? kind === 'proxy'
                                  ? proxies.isFetching
                                  : rules.isFetching
                                : kindHasError)
                            }
                            aria-label={
                              rowFeedback === 'unconfirmed'
                                ? m.dashboard_widget_provider_updates_check({
                                    name,
                                  })
                                : m.dashboard_widget_provider_updates_refresh({
                                    name,
                                  })
                            }
                            onClick={() => {
                              if (rowFeedback === 'unconfirmed')
                                handleCheck(kind, name).catch(() => undefined)
                              else handleUpdate(kind, name)
                            }}
                          >
                            <RefreshRounded className="size-4" />
                          </Button>
                        )}
                      </li>
                    )
                  })}
              </ul>
            </div>
          )}
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

export function ProviderUpdatesWidget(props: WidgetComponentProps) {
  const { disabled, isOverlay, sourceOnly } = useDndGridContext()
  if (sourceOnly || isOverlay) return <ProviderUpdatesPreview {...props} />
  return <ProviderUpdatesLive {...props} canAct={disabled && !isOverlay} />
}
