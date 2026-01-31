import Check from '~icons/material-symbols/check-rounded'
import { useCallback, useState } from 'react'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { Wheel } from '@uiw/react-color'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function ThemeColorConfig() {
  const { themeColor, setThemeColor } = useExperimentalThemeContext()

  const [open, setOpen] = useState(false)

  const [cachedThemeColor, setCachedThemeColor] = useState(themeColor)

  const handleSubmit = useCallback(async () => {
    setOpen(false)
    await setThemeColor(cachedThemeColor)
  }, [cachedThemeColor, setThemeColor])

  return (
    <SettingsCard data-slot="theme-color-config-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="theme-color-config-card-content"
      >
        <div>{m.settings_user_interface_theme_color_label()}</div>

        <DropdownMenu open={open} onOpenChange={setOpen}>
          <DropdownMenuTrigger asChild>
            <Button className="flex items-center gap-2 px-4" variant="flat">
              <span
                className="size-4 rounded"
                style={{ backgroundColor: themeColor }}
              />

              <span>{themeColor}</span>
            </Button>
          </DropdownMenuTrigger>

          <DropdownMenuContent className="flex flex-col gap-4 rounded-2xl p-4">
            <Wheel
              data-slot="theme-color-config-colorful"
              color={cachedThemeColor}
              onChange={(color) => {
                setCachedThemeColor(color.hex)
              }}
            />

            <Button
              className="flex items-center justify-center gap-2"
              variant="flat"
              onClick={handleSubmit}
            >
              <Check className="size-5" />
              <span>{m.common_submit()}</span>
            </Button>
          </DropdownMenuContent>
        </DropdownMenu>
      </SettingsCardContent>
    </SettingsCard>
  )
}
