import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting } from '@nyanpasu/query'
import { type TrafficRetention } from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function TrafficRetentionSelector() {
  const { value, upsert } = useSetting('traffic_retention')

  const handleChange = useLockFn(async (retention: TrafficRetention) => {
    await upsert(retention)
  })

  const messages = {
    '1d': m.settings_nyanpasu_traffic_retention_1d(),
    '7d': m.settings_nyanpasu_traffic_retention_7d(),
    '30d': m.settings_nyanpasu_traffic_retention_30d(),
    '90d': m.settings_nyanpasu_traffic_retention_90d(),
    forever: m.settings_nyanpasu_traffic_retention_forever(),
  } satisfies Record<TrafficRetention, string>

  return (
    <SettingsCard data-slot="traffic-retention-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="traffic-retention-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_nyanpasu_traffic_retention_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {value ? `${messages[value]} · ` : null}
                    {m.settings_nyanpasu_traffic_retention_hint()}
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
              onSelect={() => handleChange(key as TrafficRetention)}
            >
              {message}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
