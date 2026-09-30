import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { useClashSetting, type PortStrategyKind } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function PortStrategySelector() {
  const { value, upsert } = useClashSetting('external_controller')

  const messages = {
    allow_fallback: m.settings_clash_settings_allow_fallback_label(),
    fixed: m.settings_clash_settings_fixed_label(),
    random: m.settings_clash_settings_random_label(),
  } as Record<PortStrategyKind, string>

  const current = value?.port.kind || 'allow_fallback'

  const handlePortStrategyChange = async (kind: PortStrategyKind) => {
    await upsert({ port: { kind } })
  }

  return (
    <SettingsCard data-slot="port-strategy-selector-card">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="port-strategy-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_port_strategy_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {messages[current]}
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
              checked={current === key}
              key={key}
              onSelect={() => handlePortStrategyChange(key as PortStrategyKind)}
            >
              {message}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
