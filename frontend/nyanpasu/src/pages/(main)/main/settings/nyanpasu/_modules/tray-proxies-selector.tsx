import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { ArrowForwardIosRounded } from '@mui/icons-material'
import { ProxiesSelectorMode, useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function TrayProxiesSelector() {
  const { value, upsert } = useSetting('clash_tray_selector')

  const handleChange = useLockFn(async (mode: ProxiesSelectorMode) => {
    await upsert(mode)
  })

  const messages = {
    normal: m.settings_nyanpasu_tray_type_normal(),
    hidden: m.settings_nyanpasu_tray_type_hidden(),
    submenu: m.settings_nyanpasu_tray_type_submenu(),
  } satisfies Record<ProxiesSelectorMode, string>

  return (
    <SettingsCard data-slot="tray-proxies-selector">
      <DropdownMenu width="full">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="tray-proxies-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_nyanpasu_tray_type()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {value ? messages[value] : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent>
          {Object.entries(messages).map(([key, value]) => (
            <DropdownMenuItem
              key={key}
              onSelect={() => handleChange(key as ProxiesSelectorMode)}
            >
              {value}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
