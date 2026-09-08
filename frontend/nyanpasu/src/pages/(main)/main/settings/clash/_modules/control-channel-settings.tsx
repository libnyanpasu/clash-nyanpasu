import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Switch } from '@/components/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card'

export default function ControlChannelSettings() {
  const channel = useSetting('clash_control_channel')
  const disableHttp = useSetting('clash_ipc_disable_http_controller')
  const current = channel.value ?? 'prefer_ipc'
  const options = {
    prefer_ipc: m.settings_clash_control_channel_prefer_ipc(),
    http_only: m.settings_clash_control_channel_http_only(),
  }
  const reportError = (error: unknown) =>
    message(formatError(error), { title: 'Error', kind: 'error' })
  return (
    <div>
      <SettingsLabel>{m.settings_clash_control_channel_label()}</SettingsLabel>
      <SettingsGroup>
        <SettingsCard>
          <DropdownMenu align="end">
            <DropdownMenuTrigger asChild>
              <SettingsCardContent asChild>
                <Button
                  disabled={channel.isPending}
                  className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
                >
                  <ItemContainer>
                    <ItemLabel>
                      <ItemLabelText>
                        {m.settings_clash_control_channel_label()}
                      </ItemLabelText>
                      <ItemLabelDescription>
                        {options[current]}
                      </ItemLabelDescription>
                    </ItemLabel>
                    <ArrowForwardIosRounded />
                  </ItemContainer>
                </Button>
              </SettingsCardContent>
            </DropdownMenuTrigger>
            <DropdownMenuContent sideOffset={-16} alignOffset={16}>
              {(['prefer_ipc', 'http_only'] as const).map((value) => (
                <DropdownMenuCheckboxItem
                  key={value}
                  checked={value === current}
                  onSelect={() => {
                    channel
                      .upsert(value)
                      .catch(reportError)
                      .finally(() => channel.refetch())
                  }}
                >
                  {options[value]}
                </DropdownMenuCheckboxItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </SettingsCard>
        <SettingsCard>
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_clash_ipc_disable_http_label()}
                </ItemLabelText>
                <ItemLabelDescription>
                  {m.settings_clash_ipc_disable_http_description()}
                </ItemLabelDescription>
              </ItemLabel>
              <Switch
                checked={disableHttp.value ?? false}
                disabled={current === 'http_only'}
                loading={disableHttp.isPending}
                onCheckedChange={(value) => {
                  disableHttp
                    .upsert(value)
                    .catch(reportError)
                    .finally(() => disableHttp.refetch())
                }}
              />
            </ItemContainer>
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}
