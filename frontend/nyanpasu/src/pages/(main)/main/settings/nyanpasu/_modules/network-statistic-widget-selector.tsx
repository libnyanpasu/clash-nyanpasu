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
import {
  useSetting,
  type NetworkStatisticWidgetConfig,
} from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function NetworkStatisticWidgetSelector() {
  const { value, upsert } = useSetting('network_statistic_widget')

  const handleChange = useLockFn(async (mode: NetworkStatisticWidgetConfig) => {
    await upsert(mode)
  })

  const messages = {
    disabled: m.settings_nyanpasu_network_statistic_widget_disabled(),
    large: m.settings_nyanpasu_network_statistic_widget_large(),
    small: m.settings_nyanpasu_network_statistic_widget_small(),
  } satisfies Record<NetworkStatisticWidgetConfig, string>

  const current = value ?? 'disabled'

  return (
    <SettingsCard data-slot="network-statistic-widget-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="network-statistic-widget-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_nyanpasu_network_statistic_widget_label()}
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
              onSelect={() => handleChange(key as NetworkStatisticWidgetConfig)}
            >
              {message}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
