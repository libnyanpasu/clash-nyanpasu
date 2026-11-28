import { useDeferredValue, useMemo } from 'react'
import {
  ColorSchemeSwatch,
  ColorSchemeSwatchGroup,
} from '@nyanpasu/ui/color-scheme-swatch'
import type { ThemeOptions } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import type { ResolvedThemeMode, ThemeVariant } from '@nyanpasu/theme'
import { THEME_STYLE_MESSAGES } from '../../_modules/theme-style-messages'
import { ThemeSection } from './theme-section'
import { themeSwatchColors } from './theme-swatches'

export function ThemeStyleSection({
  className,
  draft,
  swatchMode,
  onChange,
}: {
  className?: string
  draft: ThemeOptions
  swatchMode: ResolvedThemeMode
  onChange: (variant: ThemeVariant) => void
}) {
  // Each style is shown as the scheme it generates from the source color.
  const { color, contrastLevel, specVersion } = useDeferredValue(draft)

  const styles = useMemo(
    () =>
      THEME_STYLE_MESSAGES.map((style) => ({
        ...style,
        colors: themeSwatchColors(
          { color, variant: style.variant, contrastLevel, specVersion },
          swatchMode,
        ),
      })),
    [color, contrastLevel, specVersion, swatchMode],
  )

  const selected = styles.find((style) => style.variant === draft.variant)

  return (
    <ThemeSection
      data-slot="theme-style-section"
      className={className}
      title={m.settings_user_interface_theme_style_title()}
      description={m.settings_user_interface_theme_style_description()}
    >
      <ColorSchemeSwatchGroup
        aria-label={m.settings_user_interface_theme_style_title()}
        value={draft.variant}
        onValueChange={(variant) => onChange(variant as ThemeVariant)}
      >
        {styles.map((style) => (
          <ColorSchemeSwatch
            key={style.variant}
            value={style.variant}
            label={style.label()}
            colors={style.colors}
          />
        ))}
      </ColorSchemeSwatchGroup>

      {selected && (
        <div
          data-slot="theme-style-summary"
          className="bg-surface-container dark:bg-surface-container-high rounded-large flex flex-col gap-0.5 px-4 py-3"
        >
          <span className="text-on-surface text-sm font-medium">
            {selected.label()}
          </span>

          <span className="text-on-surface-variant text-xs leading-5">
            {selected.summary()}
          </span>
        </div>
      )}
    </ThemeSection>
  )
}
