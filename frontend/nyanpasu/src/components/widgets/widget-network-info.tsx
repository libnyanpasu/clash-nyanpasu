import ErrorOutlineRounded from '~icons/material-symbols/error-outline-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { useReducedMotion } from 'motion/react'
import { useEffect, useState, type ComponentType, type ReactNode } from 'react'
import { ActionSwap } from '@nyanpasu/ui/action-swap-text'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { Spoiler } from '@nyanpasu/ui/spoiler-text'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { IP_LOOKUP_PROVIDER_NAMES } from '@/utils/ip-lookup-provider'
import { regionName } from '@/utils/traffic-usage'
import { useRouteEgressProbe, useSetting } from '@nyanpasu/query'
import type {
  EgressAddress,
  EgressMethod,
  EgressSource,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import type { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetId } from './widget-config'
import WidgetItem from './widget-item'
import { WidgetHeader, WidgetMetric, WidgetTitle } from './widget-ui'

type Family = 'ipv4' | 'ipv6'

const FAMILIES: readonly Family[] = ['ipv4', 'ipv6']

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

function methodLabel(method: EgressMethod) {
  switch (method) {
    case 'system_proxy':
      return m.dashboard_widget_network_info_method_system_proxy()
    case 'tun':
      return m.dashboard_widget_network_info_method_tun()
    case 'transparent_proxy':
      return m.dashboard_widget_network_info_method_transparent_proxy()
    case 'direct':
      return m.dashboard_widget_network_info_method_direct()
  }
}

function countryFlag(code: string | null | undefined): string | null {
  if (!code || !/^[a-z]{2}$/i.test(code)) return null
  return String.fromCodePoint(
    ...code
      .toUpperCase()
      .split('')
      .map((char) => 127397 + char.charCodeAt(0)),
  )
}

function RefreshButton({
  disabled,
  loading,
  onClick,
  label,
}: {
  disabled: boolean
  loading: boolean
  onClick?: () => void
  label: string
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="basic"
          icon
          className="text-primary size-7 shrink-0"
          aria-label={label}
          disabled={disabled}
          loading={loading}
          onClick={onClick}
          data-slot="network-info-refresh"
        >
          <RefreshRounded className="size-4" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

function FlagBadge({ country }: { country: string | null | undefined }) {
  const flag = countryFlag(country)

  return (
    <div
      data-slot="network-info-flag-container"
      className="relative w-10 shrink-0 self-start"
      aria-hidden="true"
    >
      {flag ? (
        <>
          <span
            data-slot="network-info-flag"
            className="text-[40px] leading-none"
          >
            {flag}
          </span>

          <span
            data-slot="network-info-flag-glow"
            className="absolute inset-0 -z-10 text-[40px] opacity-20 blur-lg"
          >
            {flag}
          </span>
        </>
      ) : (
        <PublicRounded className="size-6" />
      )}
    </div>
  )
}

function EgressDetails({
  method,
  ipv4,
  ipv6,
  hideAddresses,
}: {
  method: EgressMethod
  ipv4: EgressAddress<string> | null
  ipv6: EgressAddress<string> | null
  hideAddresses: boolean
}) {
  const reducedMotion = useReducedMotion()
  const [familyIndex, setFamilyIndex] = useState(0)

  const activeFamily = FAMILIES[familyIndex % FAMILIES.length]
  const activeEgress = activeFamily === 'ipv4' ? ipv4 : ipv6
  const primary = activeEgress ?? ipv4 ?? ipv6

  const countryLabel = primary?.country
    ? regionName(primary.country)
    : m.dashboard_widget_network_info_no_address()
  const route = [methodLabel(method), primary && sourceLabel(primary.source)]
    .filter(Boolean)
    .join(' · ')

  const addressText = activeEgress
    ? activeEgress.address
    : activeFamily === 'ipv4'
      ? m.dashboard_widget_network_info_no_ipv4()
      : m.dashboard_widget_network_info_no_ipv6()

  const revealLabel = m.dashboard_widget_network_info_reveal_address()

  useEffect(() => {
    if (reducedMotion) return

    // Leave enough time to read the address before the next family.
    const duration = Math.max(
      6000,
      Math.ceil(addressText.length / 30) * 1000 + 4000,
    )
    const interval = window.setInterval(() => {
      if (!document.hidden) {
        setFamilyIndex((current) => (current + 1) % FAMILIES.length)
      }
    }, duration)

    return () => {
      window.clearInterval(interval)
    }
  }, [reducedMotion, activeFamily, addressText])

  return (
    <div
      className="flex min-h-0 flex-1 items-center gap-4"
      data-slot="network-info-details"
    >
      <FlagBadge country={primary?.country} />

      <div className="flex h-full min-w-0 flex-1 flex-col justify-between gap-0.5">
        <div
          className="flex min-w-0 flex-col justify-center gap-1.5"
          data-slot="network-info-hero"
        >
          <WidgetMetric className="min-w-0 text-2xl leading-tight font-bold">
            <TextMarquee>{countryLabel}</TextMarquee>
          </WidgetMetric>

          <TextMarquee className="text-on-surface-variant text-xs leading-tight">
            {route}
          </TextMarquee>
        </div>

        <div className="flex min-w-0" data-slot="widget-network-info-rows">
          <ActionSwap
            contentKey={addressText}
            className="min-h-5 min-w-0 flex-1 [&>div]:min-w-0"
            data-slot="network-info-address-swap"
          >
            {activeEgress ? (
              <Spoiler
                key={`${addressText}:${hideAddresses ? 'hidden' : 'shown'}`}
                className="font-mono text-sm leading-tight font-medium select-all"
                defaultRevealed={!hideAddresses}
                label={revealLabel}
              >
                {addressText}
              </Spoiler>
            ) : (
              <p
                className="text-on-surface-variant min-w-0 truncate text-sm leading-tight"
                data-slot="network-info-address"
              >
                {addressText}
              </p>
            )}
          </ActionSwap>
        </div>
      </div>
    </div>
  )
}

function NetworkInfoLoading() {
  const reducedMotion = useReducedMotion()
  const pulseClass = reducedMotion ? '' : 'animate-pulse'

  return (
    <div
      className="flex min-h-0 flex-1 items-center gap-4 overflow-hidden"
      data-slot="network-info-loading"
    >
      <div
        className={cn(
          'bg-primary-container/50 flex h-10 w-14 shrink-0 items-center justify-center rounded-xl',
          pulseClass,
        )}
      >
        <CircularProgress
          indeterminate
          className="text-primary size-5"
          aria-hidden="true"
        />
      </div>

      <div className="flex min-w-0 flex-1 flex-col justify-center gap-2">
        <p
          className="text-on-surface text-base leading-tight font-semibold"
          role="status"
        >
          {m.dashboard_widget_network_info_probing()}
        </p>

        <div
          className={cn(
            'bg-surface-variant/45 h-3.5 w-3/4 rounded-full',
            pulseClass,
          )}
        />

        <div className="flex items-center gap-2">
          <div
            className={cn(
              'bg-secondary-container/50 h-4 w-9 shrink-0 rounded-md',
              pulseClass,
            )}
          />
          <div
            className={cn(
              'bg-surface-variant/35 h-3.5 w-1/2 rounded-full',
              pulseClass,
            )}
          />
        </div>
      </div>
    </div>
  )
}

function NetworkInfoNotice({
  tone,
  icon: Icon,
  role,
  message,
  action,
  refreshButton,
}: {
  tone: 'neutral' | 'error'
  icon: ComponentType<{ className?: string }>
  role: 'status' | 'alert'
  message: string
  action?: ReactNode
  refreshButton?: ReactNode
}) {
  const iconBoxClass =
    tone === 'error'
      ? 'bg-error-container/70 text-error'
      : 'bg-surface-variant/50 text-on-surface-variant'

  return (
    <div
      className="flex min-h-0 flex-1 items-center gap-4 overflow-hidden"
      data-slot="network-info-notice"
    >
      <div
        className={cn(
          'flex h-10 w-14 shrink-0 items-center justify-center rounded-xl',
          iconBoxClass,
        )}
        aria-hidden="true"
      >
        <Icon className="size-6" />
      </div>

      <div className="flex min-w-0 flex-1 flex-col justify-center gap-1.5">
        <div className="flex items-start gap-2">
          <p
            className={cn(
              'line-clamp-3 min-w-0 flex-1 text-sm leading-snug font-medium',
              tone === 'error' ? 'text-error' : 'text-on-surface-variant',
            )}
            role={role}
          >
            {message}
          </p>

          {refreshButton}
        </div>

        {action && <div className="flex items-center gap-2">{action}</div>}
      </div>
    </div>
  )
}

function NetworkInfoCard({
  id,
  onCloseClick,
  refreshButton,
  children,
}: WidgetComponentProps & {
  refreshButton?: ReactNode
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
      <Card
        className="relative isolate flex size-full flex-col"
        data-slot="network-info-card"
      >
        <WidgetHeader className="flex-row items-center gap-2 pt-3">
          <WidgetTitle className="flex-1" icon={PublicRounded}>
            {m.dashboard_widget_network_info_title()}
          </WidgetTitle>
          {refreshButton}
        </WidgetHeader>

        <CardContent className="flex min-h-0 flex-1 flex-col justify-center px-4 pt-2 pb-3">
          {children}
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

function NetworkInfoPreview(props: WidgetComponentProps) {
  return (
    <NetworkInfoCard {...props}>
      <EgressDetails
        method="direct"
        ipv4={PREVIEW_EGRESS.ipv4}
        ipv6={PREVIEW_EGRESS.ipv6}
        hideAddresses
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
  const probe = useRouteEgressProbe()

  const egress = probe.data
  const off = allowed === false || egress?.kind === 'disabled'
  const probed = egress?.kind === 'probed' ? egress : null

  const refreshLabel = m.dashboard_widget_network_info_refresh()

  const refreshButton = probed ? (
    <RefreshButton
      disabled={!canAct || probe.isPending}
      loading={probe.isPending}
      onClick={() => probe.mutate()}
      label={refreshLabel}
    />
  ) : null

  const disabledRefresh = (
    <RefreshButton disabled loading={false} label={refreshLabel} />
  )

  const content = off ? (
    <NetworkInfoNotice
      tone="neutral"
      icon={PublicRounded}
      role="status"
      message={m.dashboard_widget_network_info_disabled()}
      refreshButton={disabledRefresh}
      action={
        <Button variant="basic" className="h-7 px-2.5 text-xs" asChild>
          <Link to="/main/settings/nyanpasu/public-ip">
            {m.dashboard_widget_network_info_open_settings()}
          </Link>
        </Button>
      }
    />
  ) : probe.isError ? (
    <NetworkInfoNotice
      tone="error"
      icon={ErrorOutlineRounded}
      role="alert"
      message={m.dashboard_widget_network_info_failed()}
      action={
        <Button
          variant="basic"
          className={cn('h-7 px-2.5 text-xs', 'text-error hover:bg-error/15')}
          disabled={!canAct || probe.isPending}
          loading={probe.isPending}
          onClick={() => probe.mutate()}
          aria-label={refreshLabel}
          data-slot="network-info-refresh"
        >
          <RefreshRounded className="mr-1 inline size-3.5" />
          {refreshLabel}
        </Button>
      }
    />
  ) : !probed ? (
    <NetworkInfoLoading />
  ) : (
    <EgressDetails
      method={probed.method}
      ipv4={probed.ipv4}
      ipv6={probed.ipv6}
      hideAddresses={config.hideAddresses}
    />
  )

  return (
    <NetworkInfoCard {...props} refreshButton={refreshButton}>
      {content}
    </NetworkInfoCard>
  )
}

/**
 * The public addresses the app's own requests leave from, over the route its
 * settings give them (system proxy, TUN, transparent capture or none), with
 * the country the core's own database places them in, which route was used
 * and what answered.
 */
export function NetworkInfoWidget(props: WidgetComponentProps) {
  const { disabled, isOverlay, sourceOnly } = useDndGridContext()
  if (sourceOnly || isOverlay) return <NetworkInfoPreview {...props} />
  return <NetworkInfoLive {...props} canAct={disabled} />
}
