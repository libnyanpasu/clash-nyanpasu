import CloudSyncRounded from '~icons/material-symbols/cloud-sync-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import TextMarquee from '@nyanpasu/ui/text-marquee'
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
import {
  defineWidget,
  useWidgetConfig,
  useWidgetConfigActions,
  useWidgetInteractive,
  useWidgetListCapacity,
  Widget,
  WidgetContent,
  WidgetControls,
  WidgetHeader,
  WidgetMeta,
  WidgetTitle,
} from './core'
import {
  DEFAULT_WIDGET_CONFIGS,
  WIDGET_CONFIG_NUMBER_RANGES,
  WidgetId,
} from './widget-config'
import { ConfigChoice, ConfigStepper } from './widget-config-fields'
import { ConfigGroup } from './widget-config-row'
import { ProviderReferencesField } from './widget-reference-fields'

type ProviderKind = 'proxy' | 'rule'
type ProviderRow = {
  kind: ProviderKind
  name: string
  count: number | null | undefined
  type: string | null | undefined
  vehicleType: string | null | undefined
  updatedAt: string | null | undefined
}
type OperationFeedback = Record<string, 'failed' | 'unconfirmed'>

const providerKey = (kind: ProviderKind, name: string) => `${kind}:${name}`

function ProviderUpdatesPreview() {
  return (
    <Widget scaling="list" data-widget-kind="provider-updates">
      <WidgetHeader>
        <WidgetTitle />
      </WidgetHeader>
      <WidgetContent className="gap-2" data-slot="widget-provider-updates-card">
        <WidgetMeta>
          {m.dashboard_widget_provider_updates_counts({
            proxies: '4',
            rules: '2',
          })}
        </WidgetMeta>
        <ul
          className="flex min-h-0 flex-1 flex-col gap-1 overflow-hidden"
          data-slot="widget-provider-updates-preview-list"
        >
          {[
            { name: 'Japan Premium', kind: 'HTTP', count: '128' },
            { name: 'Regional Rules', kind: 'HTTP', count: '342' },
          ].map((provider) => (
            <li
              className="bg-surface-container-high flex h-[var(--w-row-height)] min-w-0 shrink-0 items-center gap-2 rounded-[var(--w-radius-inner)] px-2 py-1 text-[length:var(--w-text-meta)]"
              data-slot="widget-provider-updates-preview-row"
              key={provider.name}
            >
              <div className="min-w-0 flex-1">
                <p className="truncate font-medium">{provider.name}</p>
                <p className="text-on-surface-variant truncate">
                  {provider.kind} · {provider.count} ·{' '}
                  {formatRelativeTime(
                    '2026-10-10T12:58:00Z',
                    Date.parse('2026-10-10T13:00:00Z'),
                    getLocale(),
                  )}
                </p>
              </div>
              <RefreshRounded
                className="text-on-surface-variant size-4 shrink-0"
                aria-hidden="true"
              />
            </li>
          ))}
        </ul>
      </WidgetContent>
    </Widget>
  )
}

function ProviderUpdatesLive() {
  const config = useWidgetConfig(WidgetId.ProviderUpdates)
  const interactive = useWidgetInteractive()
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
  const { listRef, capacity } = useWidgetListCapacity({ baseGap: 4 })
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
    <Widget scaling="list" data-widget-kind="provider-updates">
      <WidgetHeader>
        <WidgetTitle />

        <WidgetControls>
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
                  aria-disabled={!interactive}
                  tabIndex={interactive ? 0 : -1}
                  className={
                    !interactive ? 'pointer-events-none opacity-50' : ''
                  }
                  onClick={(event) => {
                    if (!interactive) event.preventDefault()
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
        </WidgetControls>
      </WidgetHeader>

      <WidgetContent className="gap-2" data-slot="widget-provider-updates-card">
        <WidgetMeta>
          {m.dashboard_widget_provider_updates_counts({
            proxies: proxyCount,
            rules: ruleCount,
          })}
        </WidgetMeta>

        {hasError && (
          <p
            className="text-error text-[length:var(--w-text-meta)]"
            role="status"
          >
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
              className="text-error text-[length:var(--w-text-meta)] break-words"
              key={key}
              role="status"
            >
              {name}: {message}
            </p>
          )
        })}

        {isLoading && rows.length === 0 ? (
          <p
            className="text-on-surface-variant text-[length:var(--w-text-body)]"
            role="status"
          >
            {m.dashboard_widget_provider_updates_loading()}
          </p>
        ) : selectedRows.length === 0 ? (
          hasError ? (
            <p
              className="text-on-surface-variant flex-1 text-[length:var(--w-text-body)]"
              role="status"
            >
              {m.dashboard_widget_provider_updates_read_failed()}
            </p>
          ) : (
            <EmptyState
              size="compact"
              tone="normal"
              className="flex-1 overflow-hidden"
              message={m.dashboard_widget_provider_updates_empty()}
            />
          )
        ) : (
          <div
            ref={listRef}
            className="min-h-0 flex-1"
            data-slot="widget-provider-updates-list"
          >
            <ul className="flex flex-col gap-1">
              {selectedRows.slice(0, capacity).map(({ kind, name, row }) => {
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
                    className="hover:bg-on-surface/8 flex h-[var(--w-row-height)] min-w-0 shrink-0 items-center gap-2 rounded-[var(--w-radius-inner)] px-2 py-1 text-[length:var(--w-text-meta)]"
                    key={key}
                  >
                    <div className="min-w-0 flex-1">
                      {missing ? (
                        <TextMarquee className="w-full font-medium">
                          {name}
                        </TextMarquee>
                      ) : (
                        <Link
                          aria-disabled={!interactive}
                          tabIndex={interactive ? 0 : -1}
                          onClick={(event) => {
                            if (!interactive) event.preventDefault()
                          }}
                          className={`block truncate font-medium hover:underline ${!interactive ? 'pointer-events-none opacity-50' : ''}`}
                          to={to}
                          params={{ key: name }}
                        >
                          <TextMarquee className="w-full">{name}</TextMarquee>
                        </Link>
                      )}
                      <TextMarquee className="text-on-surface-variant w-full">
                        {row
                          ? `${row.vehicleType ?? '—'} · ${row.type ?? '—'} · ${row.count ?? '—'} · ${formatRelativeTime(row.updatedAt, Date.now(), getLocale())}`
                          : missing
                            ? m.dashboard_widget_provider_updates_missing()
                            : isLoading
                              ? m.dashboard_widget_provider_updates_loading()
                              : m.dashboard_widget_provider_updates_read_failed()}
                      </TextMarquee>
                    </div>
                    {row && (
                      <Button
                        className="size-8 shrink-0"
                        icon
                        loading={pending}
                        disabled={
                          !interactive ||
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
      </WidgetContent>
    </Widget>
  )
}

function ProviderUpdatesConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions(
    WidgetId.ProviderUpdates,
  )

  return (
    <>
      <ConfigGroup>
        <ConfigChoice
          label={m.dashboard_widget_config_provider_types()}
          value={config.kinds}
          options={[
            {
              value: 'both',
              label: m.dashboard_widget_config_both_providers(),
            },
            {
              value: 'proxy',
              label: m.dashboard_widget_config_proxy_providers(),
            },
            {
              value: 'rule',
              label: m.dashboard_widget_config_rule_providers(),
            },
          ]}
          disabled={disabled}
          onChange={(kinds) => updateConfig({ kinds })}
        />
        <ConfigStepper
          label={m.dashboard_widget_config_items()}
          value={config.maxItems}
          range={WIDGET_CONFIG_NUMBER_RANGES.maxItems}
          disabled={disabled}
          onChange={(maxItems) => updateConfig({ maxItems })}
        />
      </ConfigGroup>

      <ConfigGroup>
        <ProviderReferencesField
          resources={config.resources}
          disabled={disabled}
          onChange={(resources) => updateConfig({ resources })}
        />
      </ConfigGroup>
    </>
  )
}

export const providerUpdatesWidget = defineWidget({
  id: WidgetId.ProviderUpdates,
  category: 'subscription',
  title: m.dashboard_widget_provider_updates_title,
  description: m.dashboard_widget_provider_updates_description,
  icon: CloudSyncRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 3 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.ProviderUpdates],
  Component: ProviderUpdatesLive,
  Preview: ProviderUpdatesPreview,
  ConfigForm: ProviderUpdatesConfigForm,
})
