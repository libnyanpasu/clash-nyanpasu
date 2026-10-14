import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashSetting } from '@nyanpasu/query'
import type { ProxyChangeBreakMode } from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

const MODE_LABELS: Record<ProxyChangeBreakMode, () => string> = {
  off: m.settings_nyanpasu_enhance_break_when_proxy_change_off,
  proxy_group: m.settings_nyanpasu_enhance_break_when_proxy_change_group,
  all: m.settings_nyanpasu_enhance_break_when_proxy_change_all,
}

const MODES = Object.keys(MODE_LABELS) as ProxyChangeBreakMode[]

export default function BreakWhenProxyChangeSelector() {
  const breakConnection = useClashSetting('break_connection')

  const handleChange = useLockFn(async (next: ProxyChangeBreakMode) => {
    try {
      await breakConnection.upsert({ on_proxy_change: next })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  const current = breakConnection.value?.on_proxy_change

  return (
    <SettingsCard data-slot="break-when-proxy-change-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild disabled={breakConnection.isPending}>
          <SettingsCardContent
            data-slot="break-when-proxy-change-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_nyanpasu_enhance_break_when_proxy_change_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {current ? `${MODE_LABELS[current]()} · ` : null}
                    {m.settings_nyanpasu_enhance_break_when_proxy_change_description()}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {MODES.map((mode) => (
            <DropdownMenuCheckboxItem
              key={mode}
              checked={current === mode}
              onSelect={() => handleChange(mode)}
            >
              {MODE_LABELS[mode]()}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
