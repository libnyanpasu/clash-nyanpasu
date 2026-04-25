import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import Check from '~icons/material-symbols/check-rounded'
import { useCallback, useMemo, useState } from 'react'
import {
  DEFAULT_COLOR,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { Hue } from '@uiw/react-color'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

const PERSETS = [
  DEFAULT_COLOR,
  '#9e1e67',
  '#3d009e',
  '#00089e',
  '#066b9e',
  '#9e5a00',
]

function hueToHex(hue: number) {
  const h = ((hue % 360) + 360) % 360
  const c = 1
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1))

  let r = 0
  let g = 0
  let b = 0

  if (h < 60) {
    r = c
    g = x
  } else if (h < 120) {
    r = x
    g = c
  } else if (h < 180) {
    g = c
    b = x
  } else if (h < 240) {
    g = x
    b = c
  } else if (h < 300) {
    r = x
    b = c
  } else {
    r = c
    b = x
  }

  const toHex = (value: number) =>
    Math.round(value * 255)
      .toString(16)
      .padStart(2, '0')

  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

export default function ThemeColorConfig() {
  const { themeColor, setThemeColor } = useExperimentalThemeContext()

  const handleThemeModeChange = useCallback(
    (color: string) => {
      setThemeColor(color)
    },
    [setThemeColor],
  )

  const [customHueColor, setCustomHueColor] = useState<number>()

  const customColorHex = useMemo(
    () => (customHueColor !== undefined ? hueToHex(customHueColor) : undefined),
    [customHueColor],
  )

  const handleSubmit = useCallback(async () => {
    if (!customColorHex) {
      return
    }

    await setThemeColor(customColorHex)
  }, [customColorHex, setThemeColor])

  return (
    <SettingsCard data-slot="theme-color-config-card">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent data-slot="theme-mode-selector-trigger" asChild>
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
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {PERSETS.map((value) => (
            <DropdownMenuCheckboxItem
              checked={themeColor === value}
              key={value}
              onSelect={() => handleThemeModeChange(value)}
            >
              <span
                className="inline-block size-4 rounded-full"
                data-slot="theme-color-config-colorful-select-preview"
                style={{
                  backgroundColor: value,
                }}
              />

              <span>{value}</span>
            </DropdownMenuCheckboxItem>
          ))}

          <DropdownMenuSub>
            <DropdownMenuSubTrigger
              className="group justify-start gap-4"
              data-selected={String(!PERSETS.includes(themeColor))}
            >
              <Check
                className={cn(
                  'text-primary',
                  'group-data-[selected=false]:opacity-0 group-data-[selected=true]:opacity-100',
                )}
              />

              <span className="flex-1">
                {m.settings_user_interface_theme_color_custom()}
              </span>
            </DropdownMenuSubTrigger>

            <DropdownMenuSubContent>
              <div className="w-60 space-y-2 overflow-hidden p-4">
                <div className="flex items-center gap-2">
                  <span
                    className="inline-block size-4 rounded-full"
                    data-slot="theme-color-config-colorful-select-preview"
                    style={{
                      backgroundColor: customColorHex,
                    }}
                  />

                  <span>{customColorHex ?? customHueColor}</span>
                </div>

                <Hue
                  className="bg-inherit! [&>div:first-child]:rounded-full!"
                  hue={customHueColor}
                  onChange={(newHue) => {
                    setCustomHueColor(newHue.h)
                  }}
                />
              </div>

              <DropdownMenuSeparator />

              <DropdownMenuItem
                className="justify-start"
                onSelect={handleSubmit}
              >
                <Check className="size-5" />
                <span>{m.common_submit()}</span>
              </DropdownMenuItem>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
