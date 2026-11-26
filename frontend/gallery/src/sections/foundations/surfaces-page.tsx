import {
  getHctTone,
  getThemeScheme,
  hexFromArgb,
  ThemeMode,
} from '@nyanpasu/theme'
import { SURFACE_LADDER } from '@nyanpasu/theme/spec'
import { Card, CardContent, CardHeader } from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { PageHeader } from '../../studio/stage'
import { useThemeStudio } from '../../studio/theme-studio-context'

export function SurfacesPage() {
  const { palette } = useThemeStudio()
  const lightScheme = getThemeScheme(palette, ThemeMode.LIGHT)
  const darkScheme = getThemeScheme(palette, ThemeMode.DARK)

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-surfaces-page">
      <PageHeader
        eyebrow="Foundations · Surfaces"
        title="Android 17 Tonal Surface Ladder"
        description="Every adjacent region steps monotonically across M3E surface container tones so chrome, navigation, canvas, and cards separate cleanly in both light and dark modes without hairline borders or drop shadows."
        badges={['SURFACE_LADDER', `${SURFACE_LADDER.length} tiers`]}
      />

      {/* Light vs Dark Polarity Explanation */}
      <Card>
        <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
          <h2 className="text-on-surface text-base font-semibold">
            Side-by-Side Polarity Proof
          </h2>
          <p className="text-on-surface-variant text-xs leading-5">
            In light mode, pure-white surface-container-lowest cards float
            crisply over a bright surface-container-low canvas. In dark mode,
            surface-container cards lift clearly above a near-black surface
            canvas.
          </p>
        </CardHeader>

        <CardContent className="grid grid-cols-1 gap-4 px-6 pt-4 pb-6 lg:grid-cols-2">
          {(['light', 'dark'] as const).map((previewMode) => (
            <div
              key={previewMode}
              className={cn(
                'overflow-hidden rounded-2xl',
                previewMode === 'light'
                  ? 'gallery-theme-light light'
                  : 'gallery-theme-dark dark',
              )}
            >
              <div className="bg-surface-container-highest text-on-surface flex items-center justify-between px-4 py-2 text-xs font-medium">
                <span>WindowHeader ({previewMode})</span>
                <span className="text-on-surface-variant font-mono">
                  highest
                </span>
              </div>

              <div className="bg-surface-container-high dark:bg-surface-container text-on-surface flex items-center gap-2 px-4 py-2.5 text-xs">
                <span className="bg-secondary-container text-on-secondary-container rounded-full px-3 py-1 font-medium">
                  Dashboard
                </span>
                <span className="text-on-surface-variant px-2 py-1">
                  Proxies
                </span>
                <span className="text-on-surface-variant px-2 py-1">
                  Settings
                </span>
              </div>

              <div className="flex min-h-48">
                <div className="bg-surface-container dark:bg-surface-container-low flex w-28 shrink-0 flex-col gap-1.5 p-2.5">
                  <div className="bg-secondary-container text-on-secondary-container rounded-xl px-2.5 py-1.5 text-xs font-medium">
                    Active
                  </div>
                  <div className="text-on-surface-variant px-2.5 py-1.5 text-xs">
                    Idle item
                  </div>
                </div>

                <div className="bg-surface-container-low dark:bg-surface flex flex-1 flex-col gap-3 p-4">
                  <div className="bg-surface-container-lowest dark:bg-surface-container rounded-2xl p-3.5">
                    <div className="text-on-surface text-xs font-semibold">
                      Card surface
                    </div>
                    <div className="bg-surface-container dark:bg-surface-container-high text-on-surface-variant mt-2 rounded-lg p-2.5 text-[11px]">
                      Nested well inside card (outer 16px - padding 8px = 8px)
                    </div>
                  </div>
                </div>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      {/* Complete 12-Tier Specification Table */}
      <Card>
        <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
          <h2 className="text-on-surface text-base font-semibold">
            Canonical 12-Tier Surface Specification
          </h2>
          <p className="text-on-surface-variant text-xs">
            Rendered directly from SURFACE_LADDER in @nyanpasu/theme/spec.
          </p>
        </CardHeader>

        <CardContent className="flex flex-col gap-2 px-6 pt-4 pb-6">
          {SURFACE_LADDER.map((tier) => {
            const lightArgb = lightScheme[tier.lightRole]
            const darkArgb = darkScheme[tier.darkRole]
            const lightTone = Math.round(getHctTone(lightArgb))
            const darkTone = Math.round(getHctTone(darkArgb))

            return (
              <div
                key={tier.id}
                className="bg-surface-container dark:bg-surface-container-high grid grid-cols-1 items-center gap-3 rounded-2xl p-3.5 md:grid-cols-12"
              >
                <div className="md:col-span-4">
                  <div className="text-on-surface text-sm font-medium">
                    {tier.region}
                  </div>
                  <div className="text-on-surface-variant mt-0.5 text-xs">
                    {tier.usage}
                  </div>
                </div>

                <div className="flex items-center gap-2 md:col-span-3">
                  <span
                    className="border-outline-variant/50 size-7 shrink-0 rounded-lg border"
                    style={{ backgroundColor: hexFromArgb(lightArgb) }}
                  />
                  <div className="min-w-0 font-mono text-[11px]">
                    <div className="text-on-surface truncate">
                      Light · T{lightTone}
                    </div>
                    <div className="text-on-surface-variant truncate">
                      {hexFromArgb(lightArgb)}
                    </div>
                  </div>
                </div>

                <div className="flex items-center gap-2 md:col-span-3">
                  <span
                    className="border-outline-variant/50 size-7 shrink-0 rounded-lg border"
                    style={{ backgroundColor: hexFromArgb(darkArgb) }}
                  />
                  <div className="min-w-0 font-mono text-[11px]">
                    <div className="text-on-surface truncate">
                      Dark · T{darkTone}
                    </div>
                    <div className="text-on-surface-variant truncate">
                      {hexFromArgb(darkArgb)}
                    </div>
                  </div>
                </div>

                <div className="md:col-span-2">
                  <code className="bg-surface-container-lowest dark:bg-surface-container block truncate rounded-lg px-2 py-1 text-[11px]">
                    {tier.tailwindClasses}
                  </code>
                </div>
              </div>
            )
          })}
        </CardContent>
      </Card>
    </div>
  )
}
