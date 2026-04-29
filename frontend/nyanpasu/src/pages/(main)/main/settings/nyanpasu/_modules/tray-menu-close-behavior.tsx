import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { TrayMenuCloseBehavior, useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function TrayMenuCloseBehaviorSelector() {
  const { value, upsert } = useSetting('tray_menu_close_behavior')

  const handleChange = useLockFn(async (behavior: TrayMenuCloseBehavior) => {
    await upsert(behavior)
  })

  const messages = {
    hide: m.settings_nyanpasu_tray_menu_close_behavior_hide(),
    close: m.settings_nyanpasu_tray_menu_close_behavior_close(),
  } satisfies Record<TrayMenuCloseBehavior, string>

  return (
    <SettingsCard data-slot="tray-menu-close-behavior-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="tray-menu-close-behavior-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_nyanpasu_tray_menu_close_behavior()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {value ? messages[value as TrayMenuCloseBehavior] : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(messages).map(([key, message]) => (
            <DropdownMenuCheckboxItem
              checked={value === key}
              key={key}
              onSelect={() => handleChange(key as TrayMenuCloseBehavior)}
            >
              {message}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
