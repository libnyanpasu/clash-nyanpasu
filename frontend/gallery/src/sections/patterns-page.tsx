import BoltRounded from '~icons/material-symbols/bolt-rounded'
import FilterListRounded from '~icons/material-symbols/filter-list-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import RouteRounded from '~icons/material-symbols/route-rounded'
import SettingsRounded from '~icons/material-symbols/settings-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import { useState } from 'react'
import {
  Button,
  Card,
  CardContent,
  ExpressiveChoice,
  FilterChip,
  GlassControlGroup,
  NumberStepper,
  SegmentedButton,
  SegmentedButtonItem,
  ShapeToggle,
  Slider,
  Sparkline,
  Switch,
} from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { PageHeader, SpecimenCard } from '../studio/stage'

const SAMPLE_THROUGHPUT = [14, 18, 22, 19, 31, 44, 38, 52, 49, 61, 55, 68]

export function PatternsPage() {
  const [sysProxy, setSysProxy] = useState(true)
  const [tun, setTun] = useState(false)
  const [ipv6, setIpv6] = useState(true)
  const [port, setPort] = useState(7890)
  const [dockTab, setDockTab] = useState('all')
  const [widgetWidth, setWidgetWidth] = useState(340)
  const [widgetHeight, setWidgetHeight] = useState(172)
  const [proxyMode, setProxyMode] = useState('rule')

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-patterns-page">
      <PageHeader
        eyebrow="Patterns · Recipes"
        title="Composite Screen Patterns"
        description="Real multi-component compositions proving how tonal surfaces, asymmetric grouped lists, floating glass docks, and container-queried widgets work together."
        badges={['SettingsGroup', 'DataPage Dock', '@container Widgets']}
      />

      {/* Pattern 1: Android 17 Asymmetric Grouped Settings */}
      <SpecimenCard
        title="Android 17 Asymmetric Grouped Settings List"
        description="Stacked rows share outer 24px corners and tight 4px inner corners with a 4px gap revealing the canvas — zero divider clutter or card soup."
        defaultSurface="canvas"
      >
        <div className="mx-auto flex max-w-2xl flex-col gap-1">
          <div className="text-primary px-3 py-2 text-sm font-medium">
            Network &amp; Core
          </div>

          <div
            className={cn(
              'flex flex-col gap-1',
              '[&>*:first-child:not(:only-child)]:rounded-b-extra-small',
              '[&>*:last-child:not(:only-child)]:rounded-t-extra-small',
              '[&>*:not(:first-child):not(:last-child)]:rounded-extra-small',
            )}
          >
            <Card>
              <CardContent className="flex-row items-center justify-between gap-4 px-5 py-4">
                <div>
                  <div className="text-on-surface text-sm font-medium">
                    System Proxy
                  </div>
                  <div className="text-on-surface-variant text-xs">
                    Route desktop OS traffic through the local mixed port
                  </div>
                </div>
                <Switch checked={sysProxy} onCheckedChange={setSysProxy} />
              </CardContent>
            </Card>

            <Card>
              <CardContent className="flex-row items-center justify-between gap-4 px-5 py-4">
                <div>
                  <div className="text-on-surface text-sm font-medium">
                    IPv6 Stack
                  </div>
                  <div className="text-on-surface-variant text-xs">
                    Accept dual-stack DNS and outbound proxy sockets
                  </div>
                </div>
                <Switch checked={ipv6} onCheckedChange={setIpv6} />
              </CardContent>
            </Card>

            <Card>
              <CardContent className="px-5 py-4">
                <NumberStepper
                  label="Mixed Port"
                  variant="tonal"
                  layout="inline"
                  value={port}
                  min={1024}
                  max={65535}
                  decrementLabel="Decrease port"
                  incrementLabel="Increase port"
                  onChange={setPort}
                />
              </CardContent>
            </Card>
          </div>
        </div>
      </SpecimenCard>

      {/* Pattern 2: Floating Glass Dock Tool Page */}
      <SpecimenCard
        title="Data Tool Page with Floating Glass Dock"
        description="Mirrors Clash Nyanpasu's DataPage layout: calm opaque rows scrolling beneath a single frosted GlassControlGroup bottom dock."
        defaultSurface="canvas"
      >
        <div className="bg-surface-container-low dark:bg-surface rounded-large relative overflow-hidden p-4 pb-20">
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <FilterChip defaultPressed>Active (128)</FilterChip>
            <FilterChip>HTTPS</FilterChip>
            <FilterChip>Direct</FilterChip>
          </div>

          <div className="flex flex-col gap-1.5">
            {[
              {
                host: 'api.github.com:443',
                chain: 'Proxy · Tokyo-01',
                latency: '34 ms',
              },
              {
                host: 'registry.npmjs.org:443',
                chain: 'Proxy · Singapore-02',
                latency: '48 ms',
              },
              { host: 'localhost:7890', chain: 'DIRECT', latency: '1 ms' },
            ].map((row) => (
              <div
                key={row.host}
                className="bg-surface-container-lowest dark:bg-surface-container rounded-large flex items-center justify-between gap-3 px-4 py-3 text-xs"
              >
                <div className="min-w-0">
                  <div className="text-on-surface truncate font-mono font-medium">
                    {row.host}
                  </div>
                  <div className="text-on-surface-variant mt-0.5">
                    {row.chain}
                  </div>
                </div>
                <span className="text-ok shrink-0 font-mono tabular-nums">
                  {row.latency}
                </span>
              </div>
            ))}
          </div>

          <div className="pointer-events-none absolute inset-x-3 bottom-3 flex justify-center">
            <GlassControlGroup className="pointer-events-auto">
              <SegmentedButton
                variant="glass"
                size="sm"
                value={dockTab}
                onValueChange={(next) => {
                  if (next) setDockTab(next)
                }}
              >
                <SegmentedButtonItem value="all" className="px-3">
                  Live
                </SegmentedButtonItem>
                <SegmentedButtonItem value="closed" className="px-3">
                  Closed
                </SegmentedButtonItem>
              </SegmentedButton>
              <Button variant="glass" icon aria-label="Filter">
                <FilterListRounded className="size-4" />
              </Button>
              <Button variant="glass" icon aria-label="Refresh">
                <RefreshRounded className="size-4" />
              </Button>
            </GlassControlGroup>
          </div>
        </div>
      </SpecimenCard>

      {/* Pattern 3: Container-Queried Dashboard Widget (No Scrollbars) */}
      <SpecimenCard
        title="Container-Queried Dashboard Widget (Zero Internal Scrollbars)"
        description="Drag width and height to stress-test the non-negotiable dashboard widget contract: min-h-0 overflow-hidden with responsive density."
        defaultSurface="canvas"
        controls={
          <div className="grid w-full grid-cols-1 gap-4 sm:grid-cols-2">
            <div className="flex flex-col gap-1">
              <div className="flex justify-between text-xs">
                <span className="text-on-surface-variant font-medium">
                  Widget Width
                </span>
                <span className="font-mono tabular-nums">{widgetWidth}px</span>
              </div>
              <Slider
                aria-label="Widget width"
                min={220}
                max={540}
                step={10}
                value={widgetWidth}
                onValueChange={setWidgetWidth}
              />
            </div>

            <div className="flex flex-col gap-1">
              <div className="flex justify-between text-xs">
                <span className="text-on-surface-variant font-medium">
                  Widget Height
                </span>
                <span className="font-mono tabular-nums">{widgetHeight}px</span>
              </div>
              <Slider
                aria-label="Widget height"
                min={96}
                max={240}
                step={8}
                value={widgetHeight}
                onValueChange={setWidgetHeight}
              />
            </div>
          </div>
        }
      >
        <div className="flex flex-wrap items-start gap-4">
          <Card
            className="flex min-h-0 flex-col overflow-hidden"
            style={{ width: `${widgetWidth}px`, height: `${widgetHeight}px` }}
          >
            <CardContent className="min-h-0 flex-1 overflow-hidden p-3">
              <ExpressiveChoice
                label="Proxy mode"
                layout="focus"
                value={proxyMode}
                onChange={setProxyMode}
                options={[
                  {
                    value: 'rule',
                    label: 'Rule Mode',
                    description: 'Smart routing via profile',
                    icon: <RouteRounded />,
                  },
                  {
                    value: 'global',
                    label: 'Global',
                    description: 'All traffic through proxy',
                    icon: <PublicRounded />,
                  },
                  {
                    value: 'direct',
                    label: 'Direct',
                    description: 'Bypass proxy',
                    icon: <BoltRounded />,
                  },
                ]}
              />
            </CardContent>
          </Card>

          <Card
            className="flex min-h-0 flex-col overflow-hidden"
            style={{
              width: `${Math.min(widgetWidth, 320)}px`,
              height: `${widgetHeight}px`,
            }}
          >
            <CardContent className="flex min-h-0 flex-1 flex-col justify-between gap-2 overflow-hidden p-4">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <SettingsRounded className="text-primary size-4" />
                  <span className="text-on-surface text-xs font-semibold">
                    Throughput
                  </span>
                </div>
                <span className="text-primary font-mono text-sm font-semibold tabular-nums">
                  68.4 MB/s
                </span>
              </div>

              <div className="rounded-medium min-h-0 flex-1 overflow-hidden">
                <Sparkline
                  data={SAMPLE_THROUGHPUT}
                  className="text-primary size-full"
                />
              </div>

              <div className="flex h-10 shrink-0 gap-2">
                <ShapeToggle
                  compact
                  active={tun}
                  label="TUN"
                  icon={<ShieldRounded />}
                  onClick={() => setTun((v) => !v)}
                />
              </div>
            </CardContent>
          </Card>
        </div>
      </SpecimenCard>
    </div>
  )
}
