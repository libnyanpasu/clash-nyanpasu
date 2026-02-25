import Check from '~icons/material-symbols/check-rounded'
import { useCallback, useState } from 'react'
import {
  DEFAULT_COLOR,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { Wheel } from '@uiw/react-color'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const PERSETS = [
  DEFAULT_COLOR,
  '#9e1e67',
  '#3d009e',
  '#00089e',
  '#066b9e',
  '#9e5a00',
] as const

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
        className="flex items-center justify-between px-2"
        data-slot="theme-color-config-card-content"
      >
        <Card className="w-full">
          <CardHeader>
            {m.settings_user_interface_theme_color_label()}
          </CardHeader>

          <CardContent>
            <div className="flex flex-wrap gap-2">
              {PERSETS.map((color) => (
                <Button
                  key={color}
                  className="flex items-center gap-2 px-4"
                  variant={themeColor === color ? 'flat' : 'stroked'}
                  onClick={() => setThemeColor(color)}
                >
                  <span
                    className="outline-surface-variant size-4 rounded outline"
                    style={{ backgroundColor: color }}
                  />

                  <span>{color.toLocaleUpperCase()}</span>
                </Button>
              ))}
            </div>
          </CardContent>

          <CardFooter>
            <DropdownMenu open={open} onOpenChange={setOpen}>
              <DropdownMenuTrigger asChild>
                <Button className="flex items-center gap-2 px-4" variant="flat">
                  <span
                    className="outline-surface-variant size-4 rounded outline"
                    style={{
                      backgroundColor: themeColor,
                    }}
                  />

                  <span>
                    {PERSETS.includes(themeColor as (typeof PERSETS)[number])
                      ? m.settings_user_interface_theme_color_custom()
                      : themeColor.toLocaleUpperCase()}
                  </span>
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
          </CardFooter>
        </Card>
      </SettingsCardContent>
    </SettingsCard>
  )
}
