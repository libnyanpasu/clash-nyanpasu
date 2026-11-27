import ArrowForwardRounded from '~icons/material-symbols/arrow-forward-rounded'
import AutoAwesomeRounded from '~icons/material-symbols/auto-awesome-rounded'
import BoltRounded from '~icons/material-symbols/bolt-rounded'
import LayersRounded from '~icons/material-symbols/layers-rounded'
import PaletteRounded from '~icons/material-symbols/palette-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import { useState } from 'react'
import { SURFACE_LADDER, THEME_VARIANT_SPECS } from '@nyanpasu/theme/spec'
import {
  Button,
  Card,
  CardContent,
  ExpressiveChoice,
  MaterialShape,
  ShapeToggle,
} from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { getCoveredUiModules } from '../catalog/registry'
import { PageHeader } from '../studio/stage'
import { useThemeStudio } from '../studio/theme-studio-context'

export function OverviewPage({
  onNavigate,
}: {
  onNavigate: (pageId: string) => void
}) {
  const { seedColor, variant, contrastLevel, specVersion, mode } =
    useThemeStudio()
  const [heroMode, setHeroMode] = useState('rule')
  const [proxyActive, setProxyActive] = useState(true)
  const [tunActive, setTunActive] = useState(false)

  const activeVariant = THEME_VARIANT_SPECS.find((item) => item.id === variant)
  const coveredModulesCount = getCoveredUiModules().length

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-overview">
      <PageHeader
        eyebrow="Design Language as Code"
        title="Clash Nyanpasu UI Gallery"
        description="A standalone Material 3 Expressive (2025) and Android 17 workbench consuming @nyanpasu/ui and @nyanpasu/theme strictly through their public package exports. Tune seeds, MCU schemes, contrast, viewports, and states live."
        badges={[
          `@nyanpasu/theme`,
          `@nyanpasu/ui (${coveredModulesCount} modules)`,
          `spec ${specVersion}`,
        ]}
      />

      {/* Hero Expressive Showcase */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-12">
        <Card className="lg:col-span-7">
          <CardContent className="flex h-full flex-col justify-between gap-6 p-6">
            <div className="flex flex-col gap-2">
              <div className="flex items-center gap-2">
                <MaterialShape
                  depth={8}
                  className="bg-primary-container text-on-primary-container size-10 shrink-0"
                >
                  <AutoAwesomeRounded className="size-5" />
                </MaterialShape>
                <div>
                  <div className="text-on-surface text-base font-semibold">
                    Live Expressive Primitives
                  </div>
                  <div className="text-on-surface-variant text-xs">
                    Try the floating Glass Theme Studio below — every token
                    updates in real time.
                  </div>
                </div>
              </div>
            </div>

            <div className="bg-surface-container dark:bg-surface-container-high rounded-large grid grid-cols-1 gap-4 p-4 sm:grid-cols-12">
              <div className="flex h-40 flex-col sm:col-span-7">
                <ExpressiveChoice
                  label="Routing mode"
                  layout="focus"
                  value={heroMode}
                  onChange={setHeroMode}
                  options={[
                    {
                      value: 'rule',
                      label: 'Rule Mode',
                      description: 'Smart routing via active profile rules',
                      icon: <LayersRounded />,
                    },
                    {
                      value: 'global',
                      label: 'Global',
                      description: 'Route all traffic through selected proxy',
                      icon: <BoltRounded />,
                    },
                    {
                      value: 'direct',
                      label: 'Direct',
                      description: 'Bypass proxy for direct connections',
                      icon: <ShieldRounded />,
                    },
                  ]}
                />
              </div>

              <div className="grid h-40 grid-cols-2 gap-2 sm:col-span-5">
                <ShapeToggle
                  active={proxyActive}
                  label="System Proxy"
                  icon={<BoltRounded />}
                  onClick={() => setProxyActive((value) => !value)}
                />
                <ShapeToggle
                  active={tunActive}
                  label="TUN Mode"
                  icon={<ShieldRounded />}
                  onClick={() => setTunActive((value) => !value)}
                />
              </div>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex flex-wrap items-center gap-2 font-mono text-xs">
                <span className="bg-surface-container text-on-surface dark:bg-surface-container-high inline-flex items-center gap-1.5 rounded-full px-3 py-1">
                  <span
                    className="size-2.5 rounded-full"
                    style={{ backgroundColor: seedColor }}
                  />
                  {seedColor}
                </span>
                <span className="bg-surface-container text-on-surface-variant dark:bg-surface-container-high rounded-full px-3 py-1">
                  {activeVariant?.label}
                </span>
                <span className="bg-surface-container text-on-surface-variant dark:bg-surface-container-high rounded-full px-3 py-1">
                  contrast {contrastLevel.toFixed(1)}
                </span>
                <span className="bg-surface-container text-on-surface-variant dark:bg-surface-container-high rounded-full px-3 py-1">
                  mode {mode}
                </span>
              </div>

              <Button
                variant="flat"
                onClick={() => onNavigate('foundations/colors')}
              >
                <span>Explore Color System</span>
                <ArrowForwardRounded className="size-4" />
              </Button>
            </div>
          </CardContent>
        </Card>

        {/* Interactive A17 Tonal Staircase */}
        <Card className="lg:col-span-5">
          <CardContent className="flex h-full flex-col justify-between gap-4 p-6">
            <div>
              <h2 className="text-on-surface text-base font-semibold">
                A17 Monotonic Surface Staircase
              </h2>
              <p className="text-on-surface-variant mt-1 text-xs leading-5">
                Adjacent regions separate by surface tone alone — zero hairline
                borders, zero shadow soup.
              </p>
            </div>

            <div className="bg-primary-container text-on-primary-container rounded-large p-3">
              <div className="mb-2 flex items-center justify-between text-[11px] font-medium">
                <span>WindowHeader</span>
                <span className="text-on-primary-container/80 font-mono">
                  primary-container
                </span>
              </div>

              <div className="bg-secondary-container text-on-secondary-container rounded-[14px] p-3">
                <div className="mb-2 flex items-center justify-between text-[11px] font-medium">
                  <span>AppNavbar / EditorToolbar</span>
                  <span className="text-on-secondary-container/80 font-mono">
                    secondary-container
                  </span>
                </div>

                <div className="bg-surface-container dark:bg-surface-container-low rounded-[10px] p-3">
                  <div className="text-on-surface-variant mb-2 flex items-center justify-between text-[11px] font-medium">
                    <span>NavigationSidebar</span>
                    <span className="font-mono">container / low</span>
                  </div>

                  <div className="bg-surface-container-low dark:bg-surface rounded-small p-3">
                    <div className="text-on-surface-variant mb-2 flex items-center justify-between text-[11px] font-medium">
                      <span>Page Canvas</span>
                      <span className="font-mono">low / surface</span>
                    </div>

                    <div className="bg-surface-container-lowest dark:bg-surface-container rounded-small p-3">
                      <div className="text-on-surface flex items-center justify-between text-xs font-medium">
                        <span>Card Surface</span>
                        <span className="text-on-surface-variant font-mono text-[11px]">
                          lowest / container
                        </span>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <Button
              variant="stroked"
              className="w-full"
              onClick={() => onNavigate('foundations/surfaces')}
            >
              <span>Inspect all {SURFACE_LADDER.length} surface tiers</span>
              <ArrowForwardRounded className="size-4" />
            </Button>
          </CardContent>
        </Card>
      </div>

      {/* Quick navigation cards */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {[
          {
            id: 'foundations/colors',
            title: '49 M3E Roles + Contrast',
            detail:
              'Live HCT tones, WCAG ratios, fixed icon hues, and harmonized syntax tokens.',
            badge: 'Foundations',
            icon: <PaletteRounded className="size-5" />,
            chipClass: 'bg-purple-container text-on-purple-container',
          },
          {
            id: 'components/actions',
            title: `${coveredModulesCount} UI Primitives`,
            detail:
              'Interactive playgrounds, state matrices, surface switchers, and JSX snippets.',
            badge: 'Components',
            icon: <AutoAwesomeRounded className="size-5" />,
            chipClass: 'bg-blue-container text-on-blue-container',
          },
          {
            id: 'patterns',
            title: 'Composite Patterns',
            detail:
              'A17 app shell, asymmetric grouped settings, glass dock, and @container cards.',
            badge: 'Patterns',
            icon: <LayersRounded className="size-5" />,
            chipClass: 'bg-yellow-container text-on-yellow-container',
          },
          {
            id: 'guidelines',
            title: 'Living Do / Don’t Spec',
            detail:
              'Executable design rules paired with visual anti-patterns and CI guardrails.',
            badge: 'Guidelines',
            icon: <ShieldRounded className="size-5" />,
            chipClass: 'bg-red-container text-on-red-container',
          },
        ].map((item) => (
          <Card
            key={item.id}
            asChild
            className="hover:bg-surface-container dark:hover:bg-surface-container-high cursor-pointer text-left transition-colors"
          >
            <button type="button" onClick={() => onNavigate(item.id)}>
              <CardContent className="flex h-full flex-col justify-between gap-4 p-5">
                <div className="flex items-center justify-between gap-2">
                  <span
                    className={cn(
                      'rounded-large grid size-10 place-items-center',
                      item.chipClass,
                    )}
                  >
                    {item.icon}
                  </span>
                  <span className="bg-surface-container-high text-on-surface-variant rounded-full px-2.5 py-0.5 text-[11px] font-medium">
                    {item.badge}
                  </span>
                </div>

                <div>
                  <div className="text-on-surface text-sm font-semibold">
                    {item.title}
                  </div>
                  <p className="text-on-surface-variant mt-1 text-xs leading-5">
                    {item.detail}
                  </p>
                </div>
              </CardContent>
            </button>
          </Card>
        ))}
      </div>
    </div>
  )
}
