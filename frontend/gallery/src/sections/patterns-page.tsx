import AutoAwesomeRounded from '~icons/material-symbols/auto-awesome-rounded'
import BoltRounded from '~icons/material-symbols/bolt-rounded'
import BugReportRounded from '~icons/material-symbols/bug-report-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import Crop54Outline from '~icons/material-symbols/crop-5-4-outline'
import DescriptionRounded from '~icons/material-symbols/description-rounded'
import FilterListRounded from '~icons/material-symbols/filter-list-rounded'
import FolderOpenRounded from '~icons/material-symbols/folder-open-rounded'
import HelpRounded from '~icons/material-symbols/help-rounded'
import HorizontalRuleRounded from '~icons/material-symbols/horizontal-rule-rounded'
import InfoRounded from '~icons/material-symbols/info-rounded'
import LanguageRounded from '~icons/material-symbols/language-rounded'
import PaletteRounded from '~icons/material-symbols/palette-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import RouteRounded from '~icons/material-symbols/route-rounded'
import SettingsRounded from '~icons/material-symbols/settings-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import TuneRounded from '~icons/material-symbols/tune-rounded'
import { useState } from 'react'
import {
  Button,
  Card,
  CardContent,
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuPillTrigger,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  ExpressiveChoice,
  FilterChip,
  GlassControlGroup,
  Menubar,
  MenubarCheckboxItem,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarRadioGroup,
  MenubarRadioItem,
  MenubarSeparator,
  MenubarSub,
  MenubarSubContent,
  MenubarSubTrigger,
  MenubarTrigger,
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
  const [titlebarWidth, setTitlebarWidth] = useState(460)
  const [titlebarLanguage, setTitlebarLanguage] = useState('zh-cn')
  const [titlebarTheme, setTitlebarTheme] = useState('system')

  const isCompactTitlebar = titlebarWidth < 640

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-patterns-page">
      <PageHeader
        eyebrow="Patterns · Recipes"
        title="Composite Screen Patterns"
        description="Real multi-component compositions proving how tonal surfaces, asymmetric grouped lists, floating glass docks, and container-queried widgets work together."
        badges={[
          'A17 Titlebar Menu',
          'SettingsGroup',
          'DataPage Dock',
          '@container Widgets',
        ]}
      />

      {/* Pattern 0: Android 17 Adaptive Titlebar Menu */}
      <SpecimenCard
        title="Android 17 Adaptive Desktop Titlebar Menu"
        description="When horizontal space is sufficient (>= 640px), the titlebar shows static brand title + horizontal Menubar. When narrow (< 640px), the Menubar collapses into the title pill trigger (DropdownMenuPillTrigger) opening a grouped Launcher-style DropdownMenu."
        defaultSurface="canvas"
        controls={
          <div className="flex w-full flex-wrap items-center gap-4">
            <div className="flex min-w-52 flex-1 flex-col gap-1">
              <div className="flex justify-between text-xs">
                <span className="text-on-surface-variant font-medium">
                  Titlebar Width (
                  {isCompactTitlebar
                    ? 'Compact · Pill Menu'
                    : 'Expanded · Menubar'}
                  )
                </span>
                <span className="font-mono tabular-nums">
                  {titlebarWidth}px
                </span>
              </div>
              <Slider
                aria-label="Titlebar width"
                min={360}
                max={860}
                step={10}
                value={titlebarWidth}
                onValueChange={setTitlebarWidth}
              />
            </div>

            <div className="flex items-center gap-2">
              <Button
                variant={isCompactTitlebar ? 'flat' : 'stroked'}
                className="h-8 min-w-0 px-3 text-xs"
                onClick={() => setTitlebarWidth(460)}
              >
                Compact (460px)
              </Button>
              <Button
                variant={!isCompactTitlebar ? 'flat' : 'stroked'}
                className="h-8 min-w-0 px-3 text-xs"
                onClick={() => setTitlebarWidth(760)}
              >
                Expanded (760px)
              </Button>
            </div>
          </div>
        }
      >
        <div className="overflow-x-auto pb-1">
          <div
            className="border-outline-variant/40 rounded-large overflow-hidden border transition-[width] duration-200"
            style={{ width: `${titlebarWidth}px`, maxWidth: '100%' }}
          >
            <div className="bg-primary-container text-on-primary-container flex h-10 items-center justify-between gap-2 px-3">
              <div className="flex min-w-0 items-center gap-2">
                {isCompactTitlebar ? (
                  <DropdownMenu variant="launcher" align="start">
                    <DropdownMenuPillTrigger className="px-2">
                      <span className="bg-primary text-on-primary grid size-5 shrink-0 place-items-center rounded-full">
                        <AutoAwesomeRounded className="size-3.5" />
                      </span>
                      <span className="truncate text-sm font-bold text-nowrap">
                        Clash Nyanpasu
                      </span>
                    </DropdownMenuPillTrigger>

                    <DropdownMenuContent sideOffset={6}>
                      <DropdownMenuGroup>
                        <DropdownMenuItem>
                          <FolderOpenRounded className="size-5 shrink-0" />
                          <span>Import Local Profile</span>
                        </DropdownMenuItem>
                      </DropdownMenuGroup>

                      <DropdownMenuGroup>
                        <DropdownMenuSub>
                          <DropdownMenuSubTrigger>
                            <LanguageRounded className="size-5 shrink-0" />
                            <span>Language</span>
                          </DropdownMenuSubTrigger>
                          <DropdownMenuSubContent>
                            <DropdownMenuRadioGroup
                              value={titlebarLanguage}
                              onValueChange={setTitlebarLanguage}
                            >
                              <DropdownMenuRadioItem value="en">
                                English
                              </DropdownMenuRadioItem>
                              <DropdownMenuRadioItem value="zh-cn">
                                简体中文
                              </DropdownMenuRadioItem>
                              <DropdownMenuRadioItem value="ja">
                                日本語
                              </DropdownMenuRadioItem>
                            </DropdownMenuRadioGroup>
                          </DropdownMenuSubContent>
                        </DropdownMenuSub>

                        <DropdownMenuSub>
                          <DropdownMenuSubTrigger>
                            <PaletteRounded className="size-5 shrink-0" />
                            <span>Theme Mode</span>
                          </DropdownMenuSubTrigger>
                          <DropdownMenuSubContent>
                            <DropdownMenuRadioGroup
                              value={titlebarTheme}
                              onValueChange={setTitlebarTheme}
                            >
                              <DropdownMenuRadioItem value="light">
                                Light
                              </DropdownMenuRadioItem>
                              <DropdownMenuRadioItem value="dark">
                                Dark
                              </DropdownMenuRadioItem>
                              <DropdownMenuRadioItem value="system">
                                System
                              </DropdownMenuRadioItem>
                            </DropdownMenuRadioGroup>
                          </DropdownMenuSubContent>
                        </DropdownMenuSub>

                        <DropdownMenuSub>
                          <DropdownMenuSubTrigger>
                            <TuneRounded className="size-5 shrink-0" />
                            <span>Proxy Settings</span>
                          </DropdownMenuSubTrigger>
                          <DropdownMenuSubContent>
                            <DropdownMenuGroup>
                              <DropdownMenuCheckboxItem
                                checked={sysProxy}
                                onCheckedChange={setSysProxy}
                              >
                                System Proxy
                              </DropdownMenuCheckboxItem>
                              <DropdownMenuCheckboxItem
                                checked={tun}
                                onCheckedChange={setTun}
                              >
                                TUN Mode
                              </DropdownMenuCheckboxItem>
                            </DropdownMenuGroup>

                            <DropdownMenuRadioGroup
                              value={proxyMode}
                              onValueChange={setProxyMode}
                            >
                              <DropdownMenuRadioItem value="rule">
                                Rule Mode
                              </DropdownMenuRadioItem>
                              <DropdownMenuRadioItem value="global">
                                Global Mode
                              </DropdownMenuRadioItem>
                              <DropdownMenuRadioItem value="direct">
                                Direct Mode
                              </DropdownMenuRadioItem>
                            </DropdownMenuRadioGroup>
                          </DropdownMenuSubContent>
                        </DropdownMenuSub>
                      </DropdownMenuGroup>

                      <DropdownMenuGroup>
                        <DropdownMenuItem>
                          <HelpRounded className="size-5 shrink-0" />
                          <span>Wiki &amp; Docs</span>
                        </DropdownMenuItem>
                        <DropdownMenuItem>
                          <BugReportRounded className="size-5 shrink-0" />
                          <span>Report Issue</span>
                        </DropdownMenuItem>
                        <DropdownMenuItem>
                          <DescriptionRounded className="size-5 shrink-0" />
                          <span>Collect Logs</span>
                        </DropdownMenuItem>
                        <DropdownMenuItem>
                          <InfoRounded className="size-5 shrink-0" />
                          <span>About</span>
                        </DropdownMenuItem>
                      </DropdownMenuGroup>
                    </DropdownMenuContent>
                  </DropdownMenu>
                ) : (
                  <>
                    <div className="flex items-center gap-2">
                      <span className="bg-primary text-on-primary grid size-5 shrink-0 place-items-center rounded-full">
                        <AutoAwesomeRounded className="size-3.5" />
                      </span>
                      <span className="text-base font-bold text-nowrap">
                        Clash Nyanpasu
                      </span>
                    </div>

                    <Menubar
                      loop
                      className="[&_[data-slot=menubar-trigger]]:text-on-primary-container [&_[data-slot=menubar-trigger]]:hover:bg-on-primary-container/10 [&_[data-slot=menubar-trigger]]:data-[state=open]:bg-on-primary-container/10 gap-0.5 bg-transparent p-0"
                    >
                      <MenubarMenu value="file">
                        <MenubarTrigger className="min-h-8 py-1">
                          File
                        </MenubarTrigger>
                        <MenubarContent>
                          <MenubarItem>Import Local Profile</MenubarItem>
                        </MenubarContent>
                      </MenubarMenu>

                      <MenubarMenu value="settings">
                        <MenubarTrigger className="min-h-8 py-1">
                          Settings
                        </MenubarTrigger>
                        <MenubarContent>
                          <MenubarSub>
                            <MenubarSubTrigger>Language</MenubarSubTrigger>
                            <MenubarSubContent>
                              <MenubarRadioGroup
                                value={titlebarLanguage}
                                onValueChange={setTitlebarLanguage}
                              >
                                <MenubarRadioItem value="en">
                                  English
                                </MenubarRadioItem>
                                <MenubarRadioItem value="zh-cn">
                                  简体中文
                                </MenubarRadioItem>
                                <MenubarRadioItem value="ja">
                                  日本語
                                </MenubarRadioItem>
                              </MenubarRadioGroup>
                            </MenubarSubContent>
                          </MenubarSub>

                          <MenubarSub>
                            <MenubarSubTrigger>Theme Mode</MenubarSubTrigger>
                            <MenubarSubContent>
                              <MenubarRadioGroup
                                value={titlebarTheme}
                                onValueChange={setTitlebarTheme}
                              >
                                <MenubarRadioItem value="light">
                                  Light
                                </MenubarRadioItem>
                                <MenubarRadioItem value="dark">
                                  Dark
                                </MenubarRadioItem>
                                <MenubarRadioItem value="system">
                                  System
                                </MenubarRadioItem>
                              </MenubarRadioGroup>
                            </MenubarSubContent>
                          </MenubarSub>

                          <MenubarSub>
                            <MenubarSubTrigger>
                              Proxy Settings
                            </MenubarSubTrigger>
                            <MenubarSubContent>
                              <MenubarCheckboxItem
                                checked={sysProxy}
                                onCheckedChange={setSysProxy}
                              >
                                System Proxy
                              </MenubarCheckboxItem>
                              <MenubarCheckboxItem
                                checked={tun}
                                onCheckedChange={setTun}
                              >
                                TUN Mode
                              </MenubarCheckboxItem>
                              <MenubarSeparator />
                              <MenubarRadioGroup
                                value={proxyMode}
                                onValueChange={setProxyMode}
                              >
                                <MenubarRadioItem value="rule">
                                  Rule Mode
                                </MenubarRadioItem>
                                <MenubarRadioItem value="global">
                                  Global Mode
                                </MenubarRadioItem>
                                <MenubarRadioItem value="direct">
                                  Direct Mode
                                </MenubarRadioItem>
                              </MenubarRadioGroup>
                            </MenubarSubContent>
                          </MenubarSub>
                        </MenubarContent>
                      </MenubarMenu>

                      <MenubarMenu value="help">
                        <MenubarTrigger className="min-h-8 py-1">
                          Help
                        </MenubarTrigger>
                        <MenubarContent>
                          <MenubarItem>Wiki &amp; Docs</MenubarItem>
                          <MenubarItem>Report Issue</MenubarItem>
                          <MenubarItem>Collect Logs</MenubarItem>
                          <MenubarItem>About</MenubarItem>
                        </MenubarContent>
                      </MenubarMenu>
                    </Menubar>
                  </>
                )}
              </div>

              <div className="flex shrink-0 items-center gap-1 opacity-80">
                <span className="grid size-7 place-items-center rounded-full">
                  <HorizontalRuleRounded className="size-4" />
                </span>
                <span className="grid size-7 place-items-center rounded-full">
                  <Crop54Outline className="size-3.5" />
                </span>
                <span className="grid size-7 place-items-center rounded-full">
                  <CloseRounded className="size-4.5" />
                </span>
              </div>
            </div>

            <div className="bg-secondary-container text-on-secondary-container flex h-10 items-center gap-3 px-4 text-xs font-medium">
              <span className="bg-primary-container text-on-primary-container rounded-full px-3 py-1">
                Dashboard
              </span>
              <span className="opacity-75">Proxies</span>
              <span className="opacity-75">Profiles</span>
              <span className="opacity-75">Settings</span>
            </div>

            <div className="bg-surface-container-low dark:bg-surface text-on-surface-variant px-4 py-5 text-xs">
              {isCompactTitlebar
                ? 'Compact width (< 640px): hover and click the Clash Nyanpasu pill in the titlebar to open the grouped Launcher-style menu.'
                : 'Expanded width (>= 640px): horizontal Menubar sits next to the static title; drag below 640px to preview the A17 pill collapse.'}
            </div>
          </div>
        </div>
      </SpecimenCard>

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
