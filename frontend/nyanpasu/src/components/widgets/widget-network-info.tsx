import PublicRounded from '~icons/material-symbols/public-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import type { ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { IP_LOOKUP_PROVIDER_NAMES } from '@/utils/ip-lookup-provider'
import { regionName } from '@/utils/traffic-usage'
import { useClashSetting, useLocalIpProbe, useSetting } from '@nyanpasu/query'
import type { EgressAddress, EgressSource } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import type { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetId } from './widget-config'
import WidgetItem from './widget-item'
import { WidgetHeader, WidgetTitle } from './widget-ui'

type Family = 'ipv4' | 'ipv6'

const FAMILY_LABELS: Record<Family, string> = { ipv4: 'IPv4', ipv6: 'IPv6' }

const PREVIEW_EGRESS: Record<Family, EgressAddress<string>> = {
  ipv4: {
    address: '203.0.113.7',
    country: 'JP',
    source: {
      kind: 'stun',
      host: 'stun.cloudflare.com',
      port: 3478,
      transport: 'udp',
    },
  },
  ipv6: {
    address: '2001:db8::7',
    country: 'JP',
    source: {
      kind: 'stun',
      host: 'stun.cloudflare.com',
      port: 3478,
      transport: 'udp',
    },
  },
}

function sourceLabel(source: EgressSource) {
  return source.kind === 'stun'
    ? m.dashboard_widget_network_info_via_stun({
        server: `${source.host}:${source.port}`,
        transport: source.transport.toUpperCase(),
      })
    : m.dashboard_widget_network_info_via_lookup({
        provider: IP_LOOKUP_PROVIDER_NAMES[source.provider],
      })
}

/** Keeps the address's shape while hiding its digits, for a shared screen. */
const maskAddress = (address: string) => address.replace(/[0-9a-f]/gi, '•')

function EgressRow({
  family,
  egress,
  hideAddress,
}: {
  family: Family
  egress: EgressAddress<string> | null
  hideAddress: boolean
}) {
  const details = egress
    ? [egress.country && regionName(egress.country), sourceLabel(egress.source)]
        .filter(Boolean)
        .join(' · ')
    : null

  return (
    <li
      className="bg-surface-variant/20 flex min-w-0 items-center gap-3 rounded-xl px-3 py-2"
      data-slot="widget-network-info-row"
      data-family={family}
    >
      <span className="bg-secondary-container/60 text-on-secondary-container shrink-0 rounded-md px-1.5 py-0.5 text-xs font-medium">
        {FAMILY_LABELS[family]}
      </span>

      <div className="min-w-0 flex-1">
        <TextMarquee
          className={egress ? 'font-mono text-sm font-medium' : 'text-sm'}
        >
          {egress
            ? hideAddress
              ? maskAddress(egress.address)
              : egress.address
            : m.dashboard_widget_network_info_no_address()}
        </TextMarquee>

        {details && (
          <TextMarquee className="text-on-surface-variant text-xs">
            {details}
          </TextMarquee>
        )}
      </div>
    </li>
  )
}

function EgressRows({
  ipv4,
  ipv6,
  hideAddresses,
}: {
  ipv4: EgressAddress<string> | null
  ipv6: EgressAddress<string> | null
  hideAddresses: boolean
}) {
  return (
    <ul
      className="flex min-h-0 flex-col gap-1 overflow-hidden"
      data-slot="widget-network-info-rows"
    >
      <EgressRow family="ipv4" egress={ipv4} hideAddress={hideAddresses} />
      <EgressRow family="ipv6" egress={ipv6} hideAddress={hideAddresses} />
    </ul>
  )
}

function NetworkInfoCard({
  id,
  onCloseClick,
  action,
  children,
}: WidgetComponentProps & {
  action?: ReactNode
  children: ReactNode
}) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.NetworkInfo}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col" data-slot="network-info-card">
        <WidgetHeader className="flex-row items-center">
          <WidgetTitle className="flex-1" icon={PublicRounded}>
            {m.dashboard_widget_network_info_title()}
          </WidgetTitle>

          {action}
        </WidgetHeader>

        <CardContent className="min-h-0 flex-1 justify-start gap-1 overflow-hidden px-4 py-3">
          {children}
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

function NetworkInfoPreview(props: WidgetComponentProps) {
  return (
    <NetworkInfoCard {...props}>
      <EgressRows
        ipv4={PREVIEW_EGRESS.ipv4}
        ipv6={PREVIEW_EGRESS.ipv6}
        hideAddresses={false}
      />
    </NetworkInfoCard>
  )
}

function NetworkInfoLive({
  canAct,
  ...props
}: WidgetComponentProps & { canAct: boolean }) {
  const config = useWidgetConfig(props.id, WidgetId.NetworkInfo)
  const { value: allowed } = useSetting('enable_local_ip_probe')
  const { value: tunEnabled } = useClashSetting('enable_tun_mode')
  const probe = useLocalIpProbe()

  const egress = probe.data
  const off = allowed === false || egress?.kind === 'disabled'
  const tun = !off && (tunEnabled === true || egress?.kind === 'tun_enabled')
  const probed = egress?.kind === 'probed' ? egress : null

  const notice = off ? (
    <p className="text-on-surface-variant text-sm" role="status">
      <span>{m.dashboard_widget_network_info_disabled()}</span>{' '}
      <Link
        to="/main/settings/nyanpasu/public-ip"
        className="text-primary underline-offset-2 hover:underline"
      >
        {m.dashboard_widget_network_info_open_settings()}
      </Link>
    </p>
  ) : tun ? (
    <p className="text-on-surface-variant text-sm" role="status">
      {m.dashboard_widget_network_info_tun()}
    </p>
  ) : probe.isError ? (
    <p className="text-error text-sm" role="alert">
      {m.dashboard_widget_network_info_failed()}
    </p>
  ) : !probed ? (
    <p className="text-on-surface-variant text-sm" role="status">
      {m.dashboard_widget_network_info_probing()}
    </p>
  ) : null

  const refreshLabel = m.dashboard_widget_network_info_refresh()

  return (
    <NetworkInfoCard
      {...props}
      action={
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="basic"
              icon
              className="size-7 shrink-0"
              aria-label={refreshLabel}
              disabled={!canAct || off || tun || probe.isPending}
              loading={probe.isPending}
              onClick={() => probe.mutate()}
              data-slot="network-info-refresh"
            >
              <RefreshRounded className="size-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{refreshLabel}</TooltipContent>
        </Tooltip>
      }
    >
      {notice ??
        (probed && (
          <EgressRows
            ipv4={probed.ipv4}
            ipv6={probed.ipv6}
            hideAddresses={config.hideAddresses}
          />
        ))}
    </NetworkInfoCard>
  )
}

/**
 * The public addresses the core's DIRECT outbound leaves from, with the
 * country the core's own database places them in and what answered.
 */
export function NetworkInfoWidget(props: WidgetComponentProps) {
  const { disabled, isOverlay, sourceOnly } = useDndGridContext()
  if (sourceOnly || isOverlay) return <NetworkInfoPreview {...props} />
  return <NetworkInfoLive {...props} canAct={disabled} />
}
