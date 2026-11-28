import { useDeferredValue, useMemo } from 'react'
import { ThemeMockup } from '@nyanpasu/ui/theme-mockup'
import type { ThemeOptions } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { createTheme, ThemeMode, type ResolvedThemeMode } from '@nyanpasu/theme'

const PREVIEW_MODES: readonly {
  mode: ResolvedThemeMode
  label: () => string
}[] = [
  { mode: ThemeMode.LIGHT, label: m.settings_user_interface_theme_mode_light },
  { mode: ThemeMode.DARK, label: m.settings_user_interface_theme_mode_dark },
]

/** The draft painted on the app skeleton, light and dark side by side. */
export function ThemePreviewSection({ draft }: { draft: ThemeOptions }) {
  // Dragging a slider changes the draft faster than themes rebuild; let
  // React skip the intermediate ones.
  const { color, variant, contrastLevel, specVersion } = useDeferredValue(draft)

  const palette = useMemo(
    () => createTheme(color, { variant, contrastLevel, specVersion }).palette,
    [color, variant, contrastLevel, specVersion],
  )

  return (
    <div
      data-slot="theme-preview-section"
      className="grid justify-center gap-4 sm:grid-cols-[repeat(2,minmax(0,20rem))]"
    >
      {PREVIEW_MODES.map(({ mode, label }) => (
        <ThemeMockup
          key={mode}
          palette={palette}
          mode={mode}
          aria-label={m.settings_user_interface_theme_preview_mockup({
            mode: label(),
          })}
        />
      ))}
    </div>
  )
}
