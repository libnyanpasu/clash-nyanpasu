import NetworkCheckRounded from '~icons/material-symbols/network-check-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import { motion, useReducedMotion } from 'motion/react'
import { memo, useDeferredValue, useMemo } from 'react'
import { ActionSwapText } from '@nyanpasu/ui/action-swap-text'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { formatBytes } from '@/utils/format-bytes'
import { useClashConnectionDetails, useClashWSStatus } from '@nyanpasu/query'
import type { ClashConnection_Serialize } from '@nyanpasu/rpc/types'
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
  WidgetTitle,
} from './core'
import {
  DEFAULT_WIDGET_CONFIGS,
  WIDGET_CONFIG_NUMBER_RANGES,
  WidgetId,
} from './widget-config'
import {
  ConfigChoice,
  ConfigStepper,
  ConfigToggle,
} from './widget-config-fields'
import { ConfigGroup } from './widget-config-row'

function connectionTarget(connection: ClashConnection_Serialize) {
  return (
    connection.metadata?.host ||
    connection.metadata?.destinationIP ||
    connection.metadata?.remoteDestination ||
    null
  )
}

function processBasename(path: string | null | undefined) {
  if (!path) return null
  return path.split(/[\\/]/).at(-1) || path
}

const PREVIEW_CONNECTIONS = [
  {
    host: 'video.example.com',
    download: 1_572_864,
    upload: 65_536,
  },
  { host: 'api.example.com', download: 458_752, upload: 32_768 },
]

function ActiveConnectionsPreview() {
  return (
    <Widget
      scaling="list"
      data-widget-kind="active-connections"
      data-slot="widget-active-connections-card"
    >
      <WidgetHeader>
        <WidgetTitle />

        <WidgetControls>
          <span
            className="bg-surface-container-high text-on-surface-variant rounded-full px-2 py-0.5 text-[length:var(--w-text-meta)] tabular-nums"
            data-slot="widget-active-connections-count"
          >
            2
          </span>
        </WidgetControls>
      </WidgetHeader>

      <WidgetContent className="gap-[var(--w-gap-sm)]">
        <ul
          className="flex min-h-0 flex-1 flex-col gap-[var(--w-gap-sm)] overflow-hidden"
          data-slot="widget-active-connections-list"
        >
          {PREVIEW_CONNECTIONS.map((connection) => (
            <li
              className="bg-surface-container-high flex h-[var(--w-row-height)] min-w-0 shrink-0 items-center gap-[var(--w-gap-sm)] rounded-[var(--w-radius-inner)] px-3 text-[length:var(--w-text-meta)]"
              data-slot="widget-active-connections-row"
              key={connection.host}
            >
              <div className="bg-surface-container-high text-on-surface-variant rounded-medium flex size-8 shrink-0 items-center justify-center">
                <PublicRounded className="size-5" aria-hidden="true" />
              </div>
              <div className="min-w-0 flex-1">
                <div
                  className="min-w-0 truncate text-[length:var(--w-text-body)] font-medium"
                  data-slot="widget-active-connections-label"
                >
                  {connection.host}
                </div>
                <div className="text-on-surface-variant flex gap-4 tabular-nums">
                  <span className="whitespace-nowrap">
                    ↓ {formatBytes(connection.download)}/s
                  </span>
                  <span className="whitespace-nowrap">
                    ↑ {formatBytes(connection.upload)}/s
                  </span>
                </div>
              </div>
            </li>
          ))}
        </ul>
      </WidgetContent>
    </Widget>
  )
}

const ActiveConnectionsLive = memo(function ActiveConnectionsLive() {
  const config = useWidgetConfig(WidgetId.ActiveConnections)
  const interactive = useWidgetInteractive()
  const reducedMotion = useReducedMotion()
  const { data, status, connectorState, retry } = useClashConnectionDetails()
  const { isLoading: isClashStatusLoading } = useClashWSStatus()
  const details = useDeferredValue(data)
  const { listRef, capacity } = useWidgetListCapacity({
    baseGap: 8,
    reserveHeight: 24,
  })

  const connections = useMemo(() => {
    const direction = config.sort
    return [...(details?.connections ?? [])]
      .sort((left, right) => {
        const leftRate =
          direction === 'upload'
            ? left.uploadSpeed
            : direction === 'total'
              ? left.downloadSpeed + left.uploadSpeed
              : left.downloadSpeed
        const rightRate =
          direction === 'upload'
            ? right.uploadSpeed
            : direction === 'total'
              ? right.downloadSpeed + right.uploadSpeed
              : right.downloadSpeed
        return rightRate - leftRate || left.id.localeCompare(right.id)
      })
      .slice(0, config.topN)
  }, [config.sort, config.topN, details])
  const totalCount = details?.connections.length ?? 0
  const hiddenCount = totalCount - Math.min(capacity, connections.length)
  const retryDisabled =
    !interactive || connectorState !== 'connected' || status === 'connecting'

  return (
    <Widget
      scaling="list"
      data-widget-kind="active-connections"
      data-slot="widget-active-connections-card"
    >
      <WidgetHeader>
        <WidgetTitle />

        <WidgetControls>
          {details && connectorState === 'connected' && (
            <span
              className="bg-surface-container-high text-on-surface-variant rounded-full px-2 py-0.5 text-[length:var(--w-text-meta)] tabular-nums"
              data-slot="widget-active-connections-count"
            >
              {totalCount}
            </span>
          )}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="basic"
                className="size-7 shrink-0"
                icon
                aria-label={m.dashboard_widget_active_connections_open()}
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
                  to="/main/connections"
                >
                  <OpenInNewRounded className="size-4" />
                </Link>
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {m.dashboard_widget_active_connections_open()}
            </TooltipContent>
          </Tooltip>
        </WidgetControls>
      </WidgetHeader>

      <WidgetContent className="gap-[var(--w-gap-sm)]">
        {connectorState !== 'connected' ? (
          <p
            className="text-on-surface-variant text-[length:var(--w-text-body)]"
            role="status"
          >
            {isClashStatusLoading || connectorState === 'connecting'
              ? m.dashboard_widget_active_connections_connecting()
              : m.dashboard_widget_active_connections_disconnected()}
          </p>
        ) : status === 'error' ? (
          <div className="flex min-h-0 flex-1 flex-col justify-center gap-2">
            <p
              className="text-error text-[length:var(--w-text-body)]"
              role="status"
            >
              {m.dashboard_widget_active_connections_stream_error()}
            </p>
            <Button
              className="h-8 self-start px-3"
              disabled={retryDisabled}
              onClick={retry}
            >
              {m.dashboard_widget_active_connections_retry()}
            </Button>
          </div>
        ) : !details ? (
          <p
            className="text-on-surface-variant text-[length:var(--w-text-body)]"
            role="status"
          >
            {m.dashboard_widget_active_connections_waiting()}
          </p>
        ) : connections.length === 0 ? (
          <EmptyState
            size="compact"
            tone="normal"
            className="flex-1 overflow-hidden"
            message={m.dashboard_widget_active_connections_empty()}
          />
        ) : (
          <div
            ref={listRef}
            className="flex min-h-0 flex-1 flex-col overflow-hidden"
            data-slot="widget-active-connections-list"
          >
            <ul className="flex flex-col gap-[var(--w-gap-sm)]">
              {connections.slice(0, capacity).map((connection) => {
                const process = config.showProcess
                  ? processBasename(connection.metadata?.process)
                  : null
                const target = config.hideTargets
                  ? null
                  : connectionTarget(connection)

                return (
                  <motion.li
                    layout="position"
                    initial={false}
                    transition={{
                      duration: reducedMotion ? 0 : 0.22,
                      ease: 'easeOut',
                    }}
                    className="bg-surface-container-high flex h-[var(--w-row-height)] min-w-0 shrink-0 items-center gap-[var(--w-gap-sm)] rounded-[var(--w-radius-inner)] px-3 text-[length:var(--w-text-meta)]"
                    data-slot="widget-active-connections-row"
                    key={connection.id}
                  >
                    <div className="bg-surface-container-high text-on-surface-variant rounded-medium flex size-8 shrink-0 items-center justify-center">
                      <PublicRounded className="size-5" aria-hidden="true" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div
                        className="min-w-0 text-[length:var(--w-text-body)] font-medium"
                        data-slot="widget-active-connections-label"
                      >
                        <TextMarquee className="w-full" speed={30}>
                          {[target, process].filter(Boolean).join(' · ') ||
                            m.dashboard_widget_active_connections_hidden_target()}
                        </TextMarquee>
                      </div>
                      <div className="text-on-surface-variant flex gap-4 tabular-nums">
                        <ActionSwapText
                          className="whitespace-nowrap"
                          value={`↓ ${formatBytes(connection.downloadSpeed)}/s`}
                        />
                        <ActionSwapText
                          className="whitespace-nowrap"
                          value={`↑ ${formatBytes(connection.uploadSpeed)}/s`}
                        />
                      </div>
                    </div>
                  </motion.li>
                )
              })}
            </ul>
            {hiddenCount > 0 && (
              <p
                className="text-on-surface-variant mt-auto shrink-0 pt-1 text-[length:var(--w-text-meta)]"
                data-slot="widget-active-connections-remainder"
              >
                {m.dashboard_widget_active_connections_remaining({
                  count: hiddenCount,
                })}
              </p>
            )}
          </div>
        )}
      </WidgetContent>
    </Widget>
  )
})

function ActiveConnectionsConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions(
    WidgetId.ActiveConnections,
  )

  return (
    <>
      <ConfigGroup>
        <ConfigChoice
          label={m.dashboard_widget_config_sort()}
          value={config.sort}
          options={[
            {
              value: 'download',
              label: m.dashboard_widget_config_download(),
            },
            { value: 'upload', label: m.dashboard_widget_config_upload() },
            { value: 'total', label: m.dashboard_widget_config_total() },
          ]}
          disabled={disabled}
          onChange={(sort) => updateConfig({ sort })}
        />
        <ConfigStepper
          label={m.dashboard_widget_config_top()}
          value={config.topN}
          range={WIDGET_CONFIG_NUMBER_RANGES.topN}
          disabled={disabled}
          onChange={(topN) => updateConfig({ topN })}
        />
      </ConfigGroup>

      <ConfigGroup>
        <ConfigToggle
          label={m.dashboard_widget_config_process()}
          checked={config.showProcess}
          disabled={disabled}
          onChange={(showProcess) => updateConfig({ showProcess })}
        />
        <ConfigToggle
          label={m.dashboard_widget_config_hide_targets()}
          checked={config.hideTargets}
          disabled={disabled}
          onChange={(hideTargets) => updateConfig({ hideTargets })}
        />
      </ConfigGroup>
    </>
  )
}

export const activeConnectionsWidget = defineWidget({
  id: WidgetId.ActiveConnections,
  category: 'realtime',
  title: m.dashboard_widget_active_connections_title,
  description: m.dashboard_widget_active_connections_description,
  icon: NetworkCheckRounded,
  size: { minW: 4, minH: 2, defaultW: 6, defaultH: 3 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.ActiveConnections],
  Component: ActiveConnectionsLive,
  Preview: ActiveConnectionsPreview,
  ConfigForm: ActiveConnectionsConfigForm,
})
