import PublicRounded from '~icons/material-symbols/public-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import type { ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { IP_LOOKUP_PROVIDER_NAMES } from '@/utils/ip-lookup-provider'
import { regionName } from '@/utils/traffic-usage'
import { useClashSetting, useLocalIpProbe, useSetting } from '@nyanpasu/query'
import type { EgressAddress, EgressSource } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import {
  defineWidget,
  useWidgetConfig,
  useWidgetConfigActions,
  useWidgetInteractive,
  Widget,
  WidgetContent,
  WidgetControls,
  WidgetHeader,
  WidgetTitle,
} from './core'
import { DEFAULT_WIDGET_CONFIGS, WidgetId } from './widget-config'
import { ConfigToggle } from './widget-config-fields'
import { ConfigGroup } from './widget-config-row'

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
      className="bg-surface-variant/20 flex min-w-0 items-center gap-[var(--w-gap-sm)] rounded-[var(--w-radius-inner)] px-[var(--w-gap-sm)] py-2"
      data-slot="widget-network-info-row"
      data-family={family}
    >
      <span className="bg-secondary-container/60 text-on-secondary-container shrink-0 rounded-md px-1.5 py-0.5 text-[length:var(--w-text-meta)] font-medium">
        {FAMILY_LABELS[family]}
      </span>

      <div className="min-w-0 flex-1">
        <TextMarquee
          className={
            egress
              ? 'font-mono text-[length:var(--w-text-body)] font-medium'
              : 'text-[length:var(--w-text-body)]'
          }
        >
          {egress
            ? hideAddress
              ? maskAddress(egress.address)
              : egress.address
            : m.dashboard_widget_network_info_no_address()}
        </TextMarquee>

        {details && (
          <TextMarquee className="text-on-surface-variant text-[length:var(--w-text-meta)]">
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
      className="flex min-h-0 flex-col gap-[var(--w-gap-xs)] overflow-hidden"
      data-slot="widget-network-info-rows"
    >
      <EgressRow family="ipv4" egress={ipv4} hideAddress={hideAddresses} />
      <EgressRow family="ipv6" egress={ipv6} hideAddress={hideAddresses} />
    </ul>
  )
}

function NetworkInfoView({
  action,
  children,
}: {
  action?: ReactNode
  children: ReactNode
}) {
  return (
    <Widget scaling="list" data-widget-kind="network-info">
      <WidgetHeader>
        <WidgetTitle />

        <WidgetControls>{action}</WidgetControls>
      </WidgetHeader>

      <WidgetContent className="justify-start gap-[var(--w-gap-xs)]">
        {children}
      </WidgetContent>
    </Widget>
  )
}

function NetworkInfoPreview() {
  return (
    <NetworkInfoView>
      <EgressRows
        ipv4={PREVIEW_EGRESS.ipv4}
        ipv6={PREVIEW_EGRESS.ipv6}
        hideAddresses={false}
      />
    </NetworkInfoView>
  )
}

function NetworkInfoLive() {
  const config = useWidgetConfig(WidgetId.NetworkInfo)
  const interactive = useWidgetInteractive()
  const { value: allowed } = useSetting('enable_local_ip_probe')
  const { value: tunEnabled } = useClashSetting('enable_tun_mode')
  const { value: transparentProxy } = useClashSetting('transparent_proxy')
  const probe = useLocalIpProbe()

  const egress = probe.data
  const off = allowed === false || egress?.kind === 'disabled'
  const tun = !off && (tunEnabled === true || egress?.kind === 'tun_enabled')
  const transparent =
    !off &&
    (Boolean(
      transparentProxy?.local &&
      transparentProxy.mode != null &&
      transparentProxy.mode !== 'disabled',
    ) ||
      egress?.kind === 'transparent_proxy_enabled')
  const probed = egress?.kind === 'probed' ? egress : null

  const notice = off ? (
    <p
      className="text-on-surface-variant text-[length:var(--w-text-body)]"
      role="status"
    >
      <span>{m.dashboard_widget_network_info_disabled()}</span>{' '}
      <Link
        to="/main/settings/nyanpasu/public-ip"
        className="text-primary underline-offset-2 hover:underline"
      >
        {m.dashboard_widget_network_info_open_settings()}
      </Link>
    </p>
  ) : tun ? (
    <p
      className="text-on-surface-variant text-[length:var(--w-text-body)]"
      role="status"
    >
      {m.dashboard_widget_network_info_tun()}
    </p>
  ) : transparent ? (
    <p
      className="text-on-surface-variant text-[length:var(--w-text-body)]"
      role="status"
    >
      {m.dashboard_widget_network_info_transparent_proxy()}
    </p>
  ) : probe.isError ? (
    <p className="text-error text-[length:var(--w-text-body)]" role="alert">
      {m.dashboard_widget_network_info_failed()}
    </p>
  ) : !probed ? (
    <p
      className="text-on-surface-variant text-[length:var(--w-text-body)]"
      role="status"
    >
      {m.dashboard_widget_network_info_probing()}
    </p>
  ) : null

  const refreshLabel = m.dashboard_widget_network_info_refresh()

  return (
    <NetworkInfoView
      action={
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="basic"
              icon
              className="size-7 shrink-0"
              aria-label={refreshLabel}
              disabled={
                !interactive || off || tun || transparent || probe.isPending
              }
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
    </NetworkInfoView>
  )
}

function NetworkInfoConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions(
    WidgetId.NetworkInfo,
  )

  return (
    <ConfigGroup>
      <ConfigToggle
        label={m.dashboard_widget_network_info_config_hide_addresses()}
        checked={config.hideAddresses}
        disabled={disabled}
        onChange={(hideAddresses) => updateConfig({ hideAddresses })}
      />
    </ConfigGroup>
  )
}

/**
 * The public addresses the core's DIRECT outbound leaves from, with the
 * country the core's own database places them in and what answered.
 */
export const networkInfoWidget = defineWidget({
  id: WidgetId.NetworkInfo,
  category: 'network',
  title: m.dashboard_widget_network_info_title,
  description: m.dashboard_widget_network_info_description,
  icon: PublicRounded,
  size: { minW: 3, minH: 2, defaultW: 4, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.NetworkInfo],
  Component: NetworkInfoLive,
  Preview: NetworkInfoPreview,
  ConfigForm: NetworkInfoConfigForm,
})
