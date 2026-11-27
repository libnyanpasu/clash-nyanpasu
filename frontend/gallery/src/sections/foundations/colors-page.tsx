import {
  getContrastRatio,
  getHctTone,
  getSyntaxColors,
  getThemeScheme,
  hexFromArgb,
  ThemeMode,
  type ThemeColorRole,
} from '@nyanpasu/theme'
import {
  COLOR_ROLE_GROUPS,
  CONTRAST_PAIRS,
  FIXED_ICON_HUES,
} from '@nyanpasu/theme/spec'
import { Card, CardContent, CardHeader } from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { PageHeader } from '../../studio/stage'
import { useThemeStudio } from '../../studio/theme-studio-context'

export function ColorsPage() {
  const {
    palette,
    resolvedMode,
    seedColor,
    variant,
    contrastLevel,
    specVersion,
  } = useThemeStudio()

  const scheme = getThemeScheme(palette, resolvedMode)
  const syntax = getSyntaxColors(palette, resolvedMode)

  const contrastResults = CONTRAST_PAIRS.map((pair) => {
    const ratio = getContrastRatio(
      scheme[pair.background],
      scheme[pair.foreground],
    )
    return {
      ...pair,
      ratio,
      passesAA: ratio >= pair.minRatio,
      passesAAA: ratio >= 7.0,
      bgHex: hexFromArgb(scheme[pair.background]),
      fgHex: hexFromArgb(scheme[pair.foreground]),
    }
  })

  const renderSwatch = (
    role: ThemeColorRole,
    token: string,
    onRole?: ThemeColorRole,
  ) => {
    const argb = scheme[role]
    const hex = hexFromArgb(argb)
    const tone = Math.round(getHctTone(argb))
    const textArgb = onRole
      ? scheme[onRole]
      : tone > 55
        ? scheme.onSurface
        : scheme.surface
    const ratio = onRole ? getContrastRatio(argb, textArgb) : null

    return (
      <div
        key={role}
        className="rounded-large flex flex-col justify-between gap-3 p-3.5"
        style={{
          backgroundColor: hex,
          color: hexFromArgb(textArgb),
        }}
      >
        <div className="flex items-start justify-between gap-2">
          <span className="text-xs leading-4 font-semibold">{token}</span>
          <span className="font-mono text-[11px] opacity-80">T{tone}</span>
        </div>

        <div className="flex items-center justify-between gap-2 font-mono text-[11px] opacity-85">
          <span>{hex}</span>
          {ratio !== null && <span>{ratio.toFixed(1)}:1</span>}
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-colors-page">
      <PageHeader
        eyebrow="Foundations · Color"
        title="M3E 2025 Dynamic Color System"
        description="All 49 semantic color roles generated live by @nyanpasu/theme via Material Color Utilities DynamicScheme, paired with automated WCAG contrast auditing, fixed per-item icon hues, and seed-harmonized syntax tokens."
        badges={[
          seedColor,
          variant,
          `contrast ${contrastLevel.toFixed(1)}`,
          `spec ${specVersion}`,
          resolvedMode,
        ]}
      />

      {/* Live WCAG Contrast Audit */}
      <Card>
        <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
          <h2 className="text-on-surface text-lg font-semibold">
            Live WCAG Contrast Audit (
            {resolvedMode === ThemeMode.DARK ? 'Dark' : 'Light'})
          </h2>
          <p className="text-on-surface-variant text-sm">
            Every semantic X / on-X pair verified in real time with HCT tone
            contrast.
          </p>
        </CardHeader>

        <CardContent className="grid grid-cols-1 gap-2.5 px-6 pt-4 pb-6 sm:grid-cols-2 lg:grid-cols-3">
          {contrastResults.map((item) => (
            <div
              key={item.id}
              className="bg-surface-container dark:bg-surface-container-high rounded-large flex items-center justify-between gap-3 px-3.5 py-2.5"
            >
              <div className="flex min-w-0 items-center gap-2.5">
                <span
                  className="rounded-medium grid size-8 shrink-0 place-items-center text-xs font-bold"
                  style={{ backgroundColor: item.bgHex, color: item.fgHex }}
                >
                  Aa
                </span>
                <div className="min-w-0">
                  <div className="text-on-surface truncate font-mono text-xs font-medium">
                    {item.label}
                  </div>
                  <div className="text-on-surface-variant font-mono text-[11px] tabular-nums">
                    {item.ratio.toFixed(2)}:1 (min {item.minRatio}:1)
                  </div>
                </div>
              </div>

              <span
                className={cn(
                  'shrink-0 rounded-full px-2 py-0.5 font-mono text-[11px] font-semibold',
                  item.passesAAA
                    ? 'bg-ok/15 text-ok'
                    : item.passesAA
                      ? 'bg-secondary-container text-on-secondary-container'
                      : 'bg-error-container text-on-error-container',
                )}
              >
                {item.passesAAA ? 'AAA' : item.passesAA ? 'AA' : 'FAIL'}
              </span>
            </div>
          ))}
        </CardContent>
      </Card>

      {/* 49 M3E Color Roles */}
      {COLOR_ROLE_GROUPS.map((group) => (
        <Card key={group.id}>
          <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
            <h2 className="text-on-surface text-base font-semibold">
              {group.title}
            </h2>
            <p className="text-on-surface-variant text-xs">
              {group.description}
            </p>
          </CardHeader>

          <CardContent className="grid grid-cols-2 gap-2.5 px-6 pt-4 pb-6 sm:grid-cols-3 lg:grid-cols-5">
            {group.roles.map((entry) =>
              renderSwatch(entry.role, entry.token, entry.onRole),
            )}
          </CardContent>
        </Card>
      ))}

      {/* Fixed Per-Item Icon Hues */}
      <Card>
        <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
          <h2 className="text-on-surface text-base font-semibold">
            Fixed Per-Item Icon Hues (Non-Dynamic Exception)
          </h2>
          <p className="text-on-surface-variant text-xs">
            Stay visually distinct regardless of the chosen seed color (tones
            90/10 light, 30/90 dark).
          </p>
        </CardHeader>

        <CardContent className="grid grid-cols-2 gap-2.5 px-6 pt-4 pb-6 sm:grid-cols-4 lg:grid-cols-8">
          {FIXED_ICON_HUES.map((hue) => (
            <div
              key={hue.id}
              className={cn(
                'rounded-large flex flex-col items-center justify-center gap-1 p-4 text-center',
                hue.bgClass,
                hue.textClass,
              )}
            >
              <span className="text-sm font-semibold">{hue.label}</span>
              <span className="font-mono text-[10px] opacity-80">
                {hue.id}-container
              </span>
            </div>
          ))}
        </CardContent>
      </Card>

      {/* Harmonized Syntax Highlighting */}
      <Card>
        <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
          <h2 className="text-on-surface text-base font-semibold">
            Harmonized Syntax Palette
          </h2>
          <p className="text-on-surface-variant text-xs">
            Computed by getSyntaxColors() with Blend.harmonize() toward the
            active seed color.
          </p>
        </CardHeader>

        <CardContent className="grid grid-cols-1 gap-4 px-6 pt-4 pb-6 lg:grid-cols-12">
          <pre className="bg-surface-container-lowest rounded-large overflow-x-auto p-4 font-mono text-xs leading-6 lg:col-span-7">
            <code>
              <span style={{ color: syntax.comment }}>
                {'// Harmonized YAML / TS preview'}
              </span>
              {'\n'}
              <span style={{ color: syntax.keyword }}>export const</span>{' '}
              <span style={{ color: syntax.function }}>createTheme</span>
              <span style={{ color: syntax.punctuation }}>(</span>
              <span style={{ color: syntax.parameter }}>seed</span>
              <span style={{ color: syntax.punctuation }}>: string) {'{'}</span>
              {'\n  '}
              <span style={{ color: syntax.keyword }}>return</span>{' '}
              <span style={{ color: syntax.punctuation }}>{'{'}</span>{' '}
              <span style={{ color: syntax.parameter }}>spec</span>
              <span style={{ color: syntax.punctuation }}>:</span>{' '}
              <span style={{ color: syntax.string }}>&apos;2025&apos;</span>
              <span style={{ color: syntax.punctuation }}>,</span>{' '}
              <span style={{ color: syntax.parameter }}>contrast</span>
              <span style={{ color: syntax.punctuation }}>:</span>{' '}
              <span style={{ color: syntax.constant }}>0</span>{' '}
              <span style={{ color: syntax.punctuation }}>{'}'}</span>
              {'\n'}
              <span style={{ color: syntax.punctuation }}>{'}'}</span>
              {'\n'}
              <span style={{ color: syntax.inserted }}>
                + bg-surface-container-low
              </span>
              {'\n'}
              <span style={{ color: syntax.deleted }}>- bg-zinc-100</span>
            </code>
          </pre>

          <div className="grid grid-cols-2 gap-2 lg:col-span-5">
            {Object.entries(syntax).map(([key, color]) => (
              <div
                key={key}
                className="bg-surface-container dark:bg-surface-container-high rounded-medium flex items-center gap-2 px-3 py-2"
              >
                <span
                  className="size-3.5 shrink-0 rounded-full"
                  style={{ backgroundColor: color }}
                />
                <div className="min-w-0">
                  <div className="text-on-surface truncate font-mono text-[11px]">
                    {key}
                  </div>
                  <div className="text-on-surface-variant font-mono text-[10px]">
                    {color}
                  </div>
                </div>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
