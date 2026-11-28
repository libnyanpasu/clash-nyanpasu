import type { ColorSchemeSwatchColors } from '@nyanpasu/ui/color-scheme-swatch'
import type { ThemeOptions } from '@/components/providers/theme-provider'
import {
  createTheme,
  getThemeScheme,
  hexFromArgb,
  type ResolvedThemeMode,
} from '@nyanpasu/theme'

/** The swatch colors of the scheme `options` generate in `mode`. */
export const themeSwatchColors = (
  { color, variant, contrastLevel, specVersion }: ThemeOptions,
  mode: ResolvedThemeMode,
): ColorSchemeSwatchColors => {
  const scheme = getThemeScheme(
    createTheme(color, { variant, contrastLevel, specVersion }).palette,
    mode,
  )

  return {
    primary: hexFromArgb(scheme.primary),
    secondary: hexFromArgb(scheme.secondaryContainer),
    tertiary: hexFromArgb(scheme.tertiary),
    surface: hexFromArgb(scheme.surfaceContainer),
  }
}
