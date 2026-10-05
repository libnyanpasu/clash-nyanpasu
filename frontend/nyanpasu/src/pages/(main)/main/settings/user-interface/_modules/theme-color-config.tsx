import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import BrightnessMediumRounded from '~icons/material-symbols/brightness-medium-rounded'
import DarkModeRounded from '~icons/material-symbols/dark-mode-rounded'
import LightModeRounded from '~icons/material-symbols/light-mode-rounded'
import { useEffect, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  HctColorPicker,
  HctColorPickerHexField,
  HctColorPickerPreset,
  HctColorPickerPresets,
  HctColorPickerSlider,
  HctColorPickerSliders,
} from '@nyanpasu/ui/hct-color-picker'
import {
  Popover,
  PopoverAnchor,
  PopoverContent,
  PopoverTrigger,
} from '@nyanpasu/ui/popover'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSystemAccentColor } from '@nyanpasu/query'
import { ThemeMode } from '@nyanpasu/theme'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

const PERSETS = [
  '#fa89af',
  '#FF8998',
  '#F585D3',
  '#D490FF',
  '#B89CFF',
  '#3d009e',
  '#00089e',
  '#066b9e',
  '#3e64a7',
  '#4CC4E1',
  '#33ccbb',
  '#61cb93',
  '#f3a81e',
  '#ff9565',
]

// The system accent color arrives as uppercase `#RRGGBB`; the picker emits
// lowercase.
const isSameColor = (a: string, b: string) =>
  a.toLowerCase() === b.toLowerCase()

export default function ThemeColorConfig() {
  const { themeColor, themeMode, setThemeColor, setThemePreview } =
    useExperimentalThemeContext()

  const { systemAccentColor } = useSystemAccentColor()

  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState(themeColor)
  const [previewMode, setPreviewMode] = useState<ThemeMode>()
  const [applying, setApplying] = useState(false)
  const [closeWhenSaved, setCloseWhenSaved] = useState(false)

  useEffect(() => {
    setThemePreview(open ? { color: draft, mode: previewMode } : null)
  }, [open, draft, previewMode, setThemePreview])

  // Leaving the page with the panel open must not keep the preview.
  useEffect(() => () => setThemePreview(null), [setThemePreview])

  // `upsert` resolves before the saved value is refetched; closing (and so
  // dropping the preview) any earlier would flash the old color.
  useEffect(() => {
    if (closeWhenSaved && isSameColor(themeColor, draft)) {
      setCloseWhenSaved(false)
      setOpen(false)
    }
  }, [closeWhenSaved, themeColor, draft])

  const modes = [
    {
      value: ThemeMode.DARK,
      label: m.settings_user_interface_theme_mode_dark(),
      Icon: DarkModeRounded,
    },
    {
      value: ThemeMode.SYSTEM,
      label: m.settings_user_interface_theme_mode_system(),
      Icon: BrightnessMediumRounded,
    },
    {
      value: ThemeMode.LIGHT,
      label: m.settings_user_interface_theme_mode_light(),
      Icon: LightModeRounded,
    },
  ]

  const handleOpenChange = (nextOpen: boolean) => {
    // Closing mid-save would drop the preview before the color is saved.
    if (applying) {
      return
    }

    if (nextOpen) {
      setDraft(themeColor)
      setPreviewMode(undefined)
    }

    setCloseWhenSaved(false)
    setOpen(nextOpen)
  }

  const handleDraftChange = (color: string) => {
    setDraft(color)
    setCloseWhenSaved(false)
  }

  const handleModeChange = (value: string) => {
    // Clicking the selected mode again deselects it; keep one selected.
    if (value) {
      setPreviewMode(value as ThemeMode)
    }
  }

  const handleApply = async () => {
    setApplying(true)

    try {
      await setThemeColor(draft)
      setCloseWhenSaved(true)
    } catch (error) {
      message(`Update theme color failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    } finally {
      setApplying(false)
    }
  }

  return (
    <SettingsCard className="relative" data-slot="theme-color-config-card">
      <Popover open={open} onOpenChange={handleOpenChange}>
        <PopoverTrigger asChild>
          <SettingsCardContent data-slot="theme-color-config-trigger" asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_user_interface_theme_color_label()}
                  </ItemLabelText>

                  <ItemLabelDescription className="space-x-1.5">
                    <span
                      className="bg-primary inline-block size-3 rounded-full"
                      data-slot="theme-color-config-colorful-preview"
                      style={{
                        backgroundColor: themeColor,
                      }}
                    />

                    <span>{themeColor}</span>
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </PopoverTrigger>

        {/* Must follow the trigger: Radix registers the trigger as the anchor
            until it sees a custom one, so this registration has to run last. */}
        <PopoverAnchor className="pointer-events-none absolute right-0 bottom-0 size-0" />

        {/* A left placement makes Radix slide the panel vertically near the
            window bottom instead of capping its height under the row. */}
        <PopoverContent
          side="left"
          align="start"
          sideOffset={0}
          alignOffset={8}
          sticky="always"
          aria-label={m.settings_user_interface_theme_color_label()}
          className="w-78"
        >
          <HctColorPicker
            className="min-h-0 gap-3 overflow-y-auto p-4 *:shrink-0"
            value={draft}
            onValueChange={handleDraftChange}
          >
            <HctColorPickerPresets
              aria-label={m.settings_user_interface_theme_color_presets()}
            >
              {PERSETS.map((value) => (
                <HctColorPickerPreset key={value} value={value} />
              ))}

              {systemAccentColor && (
                <HctColorPickerPreset
                  value={systemAccentColor}
                  label={m.settings_user_interface_theme_color_system_accent()}
                />
              )}
            </HctColorPickerPresets>

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

            <SegmentedButton
              data-slot="theme-color-config-mode-preview"
              aria-label={m.settings_user_interface_theme_mode_label()}
              value={previewMode ?? themeMode}
              onValueChange={handleModeChange}
            >
              {modes.map(({ value, label, Icon }) => (
                <SegmentedButtonItem
                  key={value}
                  value={value}
                  aria-label={label}
                  title={label}
                >
                  <Icon className="size-5" />
                </SegmentedButtonItem>
              ))}
            </SegmentedButton>

            <Button
              data-slot="theme-color-config-apply"
              className="self-end"
              variant="flat"
              disabled={applying || isSameColor(draft, themeColor)}
              loading={applying}
              onClick={handleApply}
            >
              {m.common_apply()}
            </Button>
          </HctColorPicker>
        </PopoverContent>
      </Popover>
    </SettingsCard>
  )
}
