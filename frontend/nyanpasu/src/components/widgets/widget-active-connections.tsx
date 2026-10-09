import NetworkCheckRounded from '~icons/material-symbols/network-check-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import { filesize } from 'filesize'
import { motion, useReducedMotion } from 'motion/react'
import { memo, useDeferredValue, useMemo } from 'react'
import { ActionSwapText } from '@nyanpasu/ui/action-swap-text'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { useClashConnectionDetails, useClashWSStatus } from '@nyanpasu/query'
import type { ClashConnection_Serialize } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { useWidgetHeight } from './use-widget-height'
import { WidgetId } from './widget-config'
import WidgetItem from './widget-item'
import { WidgetTitle } from './widget-ui'

const CONNECTION_ROW_HEIGHT = 60
const CONNECTION_REMAINDER_HEIGHT = 24

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

function ActiveConnectionsPreview({ id, onCloseClick }: WidgetComponentProps) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ActiveConnections}
      minW={4}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-active-connections-card">
        <CardContent className="flex size-full min-h-0 flex-col gap-2 overflow-hidden">
          <div className="flex shrink-0 items-center gap-2">
            <WidgetTitle className="flex-1" icon={NetworkCheckRounded}>
              {m.dashboard_widget_active_connections_title()}
            </WidgetTitle>
            <span
              className="bg-surface-variant/30 text-on-surface-variant rounded-full px-2 py-0.5 text-xs tabular-nums"
              data-slot="widget-active-connections-count"
            >
              2
            </span>
          </div>
          <ul
            className="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden"
            data-slot="widget-active-connections-list"
          >
            {[
              {
                host: 'video.example.com',
                download: 1_572_864,
                upload: 65_536,
              },
              { host: 'api.example.com', download: 458_752, upload: 32_768 },
            ].map((connection) => (
              <li
                className="bg-surface-variant/30 flex min-w-0 shrink-0 items-center gap-3 rounded-2xl px-3 py-2 text-xs"
                data-slot="widget-active-connections-row"
                key={connection.host}
              >
                <div className="bg-surface-variant/40 text-on-surface-variant flex size-8 shrink-0 items-center justify-center rounded-xl">
                  <PublicRounded className="size-5" aria-hidden="true" />
                </div>
                <div className="min-w-0 flex-1">
                  <div
                    className="min-w-0 truncate text-sm font-medium"
                    data-slot="widget-active-connections-label"
                  >
                    {connection.host}
                  </div>
                  <div className="text-on-surface-variant flex gap-4 tabular-nums">
                    <span className="whitespace-nowrap">
                      ↓ {filesize(connection.download, { standard: 'iec' })}/s
                    </span>
                    <span className="whitespace-nowrap">
                      ↑ {filesize(connection.upload, { standard: 'iec' })}/s
                    </span>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

const ActiveConnectionsLive = memo(function ActiveConnectionsLive({
  id,
  onCloseClick,
  canAct,
}: WidgetComponentProps & { canAct: boolean }) {
  const config = useWidgetConfig(id, WidgetId.ActiveConnections)
  const reducedMotion = useReducedMotion()
  const { data, status, connectorState, retry } = useClashConnectionDetails()
  const { isLoading: isClashStatusLoading } = useClashWSStatus()
  const details = useDeferredValue(data)
  const { ref: listRef, height: listHeight } = useWidgetHeight<HTMLDivElement>()

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
  const rowHeight = CONNECTION_ROW_HEIGHT
  const capacity =
    listHeight == null
      ? 1
      : Math.max(1, Math.floor((listHeight + 8) / rowHeight))
  const totalCount = details?.connections.length ?? 0
  const visibleCount =
    totalCount > Math.min(capacity, connections.length) && listHeight != null
      ? Math.max(
          1,
          Math.floor(
            (listHeight - CONNECTION_REMAINDER_HEIGHT + 8) / rowHeight,
          ),
        )
      : capacity
  const hiddenCount = totalCount - Math.min(visibleCount, connections.length)

  const retryDisabled =
    !canAct || connectorState !== 'connected' || status === 'connecting'

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ActiveConnections}
      minW={4}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-active-connections-card">
        <CardContent className="flex size-full min-h-0 flex-col gap-2 overflow-hidden">
          <div className="flex shrink-0 items-center justify-between gap-2">
            <WidgetTitle className="flex-1" icon={NetworkCheckRounded}>
              {m.dashboard_widget_active_connections_title()}
            </WidgetTitle>
            {details && connectorState === 'connected' && (
              <span
                className="bg-surface-variant/30 text-on-surface-variant rounded-full px-2 py-0.5 text-xs tabular-nums"
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
                    aria-disabled={!canAct}
                    tabIndex={canAct ? 0 : -1}
                    className={!canAct ? 'pointer-events-none opacity-50' : ''}
                    onClick={(event) => {
                      if (!canAct) event.preventDefault()
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
          </div>

          {connectorState !== 'connected' ? (
            <p className="text-on-surface-variant text-sm" role="status">
              {isClashStatusLoading || connectorState === 'connecting'
                ? m.dashboard_widget_active_connections_connecting()
                : m.dashboard_widget_active_connections_disconnected()}
            </p>
          ) : status === 'error' ? (
            <div className="flex min-h-0 flex-1 flex-col justify-center gap-2">
              <p className="text-error text-sm" role="status">
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
            <p className="text-on-surface-variant text-sm" role="status">
              {m.dashboard_widget_active_connections_waiting()}
            </p>
          ) : connections.length === 0 ? (
            <p className="text-on-surface-variant text-sm" role="status">
              {m.dashboard_widget_active_connections_empty()}
            </p>
          ) : (
            <div
              ref={listRef}
              className="flex min-h-0 flex-1 flex-col overflow-hidden"
              data-slot="widget-active-connections-list"
            >
              <ul className="flex flex-col gap-2">
                {connections.slice(0, visibleCount).map((connection) => {
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
                      className="bg-surface-variant/30 flex min-w-0 shrink-0 items-center gap-3 rounded-2xl px-3 py-2 text-xs"
                      data-slot="widget-active-connections-row"
                      key={connection.id}
                    >
                      <div className="bg-surface-variant/40 text-on-surface-variant flex size-8 shrink-0 items-center justify-center rounded-xl">
                        <PublicRounded className="size-5" aria-hidden="true" />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div
                          className="min-w-0 text-sm font-medium"
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
                            value={`↓ ${filesize(connection.downloadSpeed, { standard: 'iec' })}/s`}
                          />
                          <ActionSwapText
                            className="whitespace-nowrap"
                            value={`↑ ${filesize(connection.uploadSpeed, { standard: 'iec' })}/s`}
                          />
                        </div>
                      </div>
                    </motion.li>
                  )
                })}
              </ul>
              {hiddenCount > 0 && (
                <p
                  className="text-on-surface-variant mt-auto shrink-0 pt-1 text-xs"
                  data-slot="widget-active-connections-remainder"
                >
                  {m.dashboard_widget_active_connections_remaining({
                    count: hiddenCount,
                  })}
                </p>
              )}
            </div>
          )}
        </CardContent>
      </Card>
    </WidgetItem>
  )
})

export function ActiveConnectionsWidget(props: WidgetComponentProps) {
  const { disabled, isOverlay, sourceOnly } = useDndGridContext()
  if (sourceOnly || isOverlay) return <ActiveConnectionsPreview {...props} />
  return <ActiveConnectionsLive {...props} canAct={disabled && !isOverlay} />
}
