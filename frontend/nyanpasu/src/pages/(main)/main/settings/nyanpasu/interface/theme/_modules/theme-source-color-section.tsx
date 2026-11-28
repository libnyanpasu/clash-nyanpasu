import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import { useDeferredValue, useMemo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  ColorSchemeSwatch,
  ColorSchemeSwatchGroup,
} from '@nyanpasu/ui/color-scheme-swatch'
import {
  HctColorPicker,
  HctColorPickerHexField,
  HctColorPickerSlider,
  HctColorPickerSliders,
} from '@nyanpasu/ui/hct-color-picker'
import type { ThemeOptions } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { useSystemAccentColor } from '@nyanpasu/query'
import type { ResolvedThemeMode } from '@nyanpasu/theme'
import { ThemeSection } from './theme-section'
import { themeSwatchColors } from './theme-swatches'

const PRESETS = [
  '#fa89af',
  '#ff8998',
  '#f585d3',
  '#d490ff',
  '#b89cff',
  '#3d009e',
  '#00089e',
  '#066b9e',
  '#3e64a7',
  '#4cc4e1',
  '#33ccbb',
  '#61cb93',
  '#f3a81e',
  '#ff9565',
]

export function ThemeSourceColorSection({
  className,
  draft,
  swatchMode,
  onChange,
  onRestoreDefaults,
}: {
  className?: string
  draft: ThemeOptions
  swatchMode: ResolvedThemeMode
  onChange: (color: string) => void
  onRestoreDefaults: () => void
}) {
  const { systemAccentColor } = useSystemAccentColor()

  // Each preset is shown as the scheme it generates with the other options.
  const { variant, contrastLevel, specVersion } = useDeferredValue(draft)

  const presets = useMemo(() => {
    // The system accent color arrives as uppercase `#RRGGBB`; the picker
    // emits lowercase.
    const accent = systemAccentColor?.toLowerCase()

    return [
      ...PRESETS.map((color) => ({ color, label: color })),
      ...(accent
        ? [
            {
              color: accent,
              label: m.settings_user_interface_theme_color_system_accent(),
            },
          ]
        : []),
    ].map((preset) => ({
      ...preset,
      colors: themeSwatchColors(
        { color: preset.color, variant, contrastLevel, specVersion },
        swatchMode,
      ),
    }))
  }, [systemAccentColor, variant, contrastLevel, specVersion, swatchMode])

  const selectedPreset =
    presets.find((preset) => preset.color === draft.color.toLowerCase())
      ?.color ?? ''

  return (
    <ThemeSection
      data-slot="theme-source-color-section"
      className={className}
      title={m.settings_user_interface_theme_source_title()}
      description={m.settings_user_interface_theme_source_description()}
      action={
        <Button
          data-slot="theme-source-color-restore"
          variant="basic"
          icon
          className="size-9 shrink-0"
          aria-label={m.settings_user_interface_theme_restore_defaults()}
          title={m.settings_user_interface_theme_restore_defaults()}
          onClick={onRestoreDefaults}
        >
          <RestartAltRounded className="size-5" />
        </Button>
      }
    >
      <ColorSchemeSwatchGroup
        aria-label={m.settings_user_interface_theme_color_presets()}
        value={selectedPreset}
        onValueChange={onChange}
      >
        {presets.map((preset) => (
          <ColorSchemeSwatch
            key={preset.color}
            value={preset.color}
            label={preset.label}
            colors={preset.colors}
          />
        ))}
      </ColorSchemeSwatchGroup>

      <HctColorPicker
        className="gap-3"
        value={draft.color}
        onValueChange={onChange}
      >
        <HctColorPickerHexField
          label={m.settings_user_interface_theme_color_hex_source()}
        />

        <HctColorPickerSliders>
          <HctColorPickerSlider
            channel="hue"
            label={m.settings_user_interface_theme_color_hue()}
          />

          <HctColorPickerSlider
            channel="chroma"
            label={m.settings_user_interface_theme_color_chroma()}
          />

          <HctColorPickerSlider
            channel="tone"
            label={m.settings_user_interface_theme_color_tone()}
          />
        </HctColorPickerSliders>
      </HctColorPicker>
    </ThemeSection>
  )
}
