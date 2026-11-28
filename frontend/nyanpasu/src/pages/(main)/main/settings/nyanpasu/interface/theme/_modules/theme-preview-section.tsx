import DarkModeRounded from '~icons/material-symbols/dark-mode-rounded'
import LightModeRounded from '~icons/material-symbols/light-mode-rounded'
import SplitscreenLeftRounded from '~icons/material-symbols/splitscreen-left-rounded'
import { useDeferredValue, useMemo } from 'react'
import { Item, ItemGroup } from '@nyanpasu/ui/item-group'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { ThemeMockup } from '@nyanpasu/ui/theme-mockup'
import type { ThemeOptions } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { createTheme, ThemeMode, type ResolvedThemeMode } from '@nyanpasu/theme'
import { cn } from '@nyanpasu/utils'

export type ThemePreviewMode = ResolvedThemeMode | 'both'

const MODE_MESSAGES: Record<ResolvedThemeMode, () => string> = {
  [ThemeMode.LIGHT]: m.settings_user_interface_theme_mode_light,
  [ThemeMode.DARK]: m.settings_user_interface_theme_mode_dark,
}

export function ThemePreviewSection({
  draft,
  previewMode,
  onPreviewModeChange,
}: {
  draft: ThemeOptions
  previewMode: ThemePreviewMode
  onPreviewModeChange: (mode: ThemePreviewMode) => void
}) {
  // Dragging a slider changes the draft faster than themes rebuild; let
  // React skip the intermediate ones.
  const { color, variant, contrastLevel, specVersion } = useDeferredValue(draft)

  const palette = useMemo(
    () => createTheme(color, { variant, contrastLevel, specVersion }).palette,
    [color, variant, contrastLevel, specVersion],
  )

  const modes: ResolvedThemeMode[] =
    previewMode === 'both' ? [ThemeMode.LIGHT, ThemeMode.DARK] : [previewMode]

  return (
    <ItemGroup data-slot="theme-preview-section">
      <Item className="flex-col items-stretch">
        <div className="flex justify-center">
          <SegmentedButton
            aria-label={m.settings_user_interface_theme_preview_mode()}
            className="w-auto"
            value={previewMode}
            onValueChange={(next) => {
              // Clicking the selected mode again deselects it; keep one.
              if (next) {
                onPreviewModeChange(next as ThemePreviewMode)
              }
            }}
          >
            <SegmentedButtonItem value={ThemeMode.LIGHT} className="gap-1.5">
              <LightModeRounded className="size-4" />
              {m.settings_user_interface_theme_mode_light()}
            </SegmentedButtonItem>

            <SegmentedButtonItem value={ThemeMode.DARK} className="gap-1.5">
              <DarkModeRounded className="size-4" />
              {m.settings_user_interface_theme_mode_dark()}
            </SegmentedButtonItem>

            <SegmentedButtonItem value="both" className="gap-1.5">
              <SplitscreenLeftRounded className="size-4" />
              {m.settings_user_interface_theme_preview_both()}
            </SegmentedButtonItem>
          </SegmentedButton>
        </div>

        <div
          data-slot="theme-preview-frames"
          className={cn(
            'grid justify-center gap-4',
            previewMode === 'both'
              ? 'sm:grid-cols-[repeat(2,minmax(0,20rem))]'
              : 'grid-cols-[minmax(0,20rem)]',
          )}
        >
          {modes.map((mode) => (
            <ThemeMockup
              key={mode}
              palette={palette}
              mode={mode}
              aria-label={m.settings_user_interface_theme_preview_mockup({
                mode: MODE_MESSAGES[mode](),
              })}
            />
          ))}
        </div>
      </Item>
    </ItemGroup>
  )
}
