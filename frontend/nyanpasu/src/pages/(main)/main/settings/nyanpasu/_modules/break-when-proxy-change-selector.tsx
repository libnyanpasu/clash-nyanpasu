import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashSetting } from '@nyanpasu/query'
import type { ProxyChangeBreakMode } from '@nyanpasu/rpc/types'
import {
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

  return (
    <SettingsCard data-slot="break-when-proxy-change-selector">
      <SettingsCardContent>
        <ItemLabel>
          <ItemLabelText>
            {m.settings_nyanpasu_enhance_break_when_proxy_change_label()}
          </ItemLabelText>

          <ItemLabelDescription>
            {m.settings_nyanpasu_enhance_break_when_proxy_change_description()}
          </ItemLabelDescription>
        </ItemLabel>

        <SegmentedButton
          data-slot="break-when-proxy-change-selector-options"
          aria-label={m.settings_nyanpasu_enhance_break_when_proxy_change_label()}
          value={breakConnection.value?.on_proxy_change}
          onValueChange={(next) => {
            if (next) handleChange(next as ProxyChangeBreakMode)
          }}
          disabled={breakConnection.isPending}
        >
          {MODES.map((mode) => (
            <SegmentedButtonItem key={mode} value={mode}>
              {MODE_LABELS[mode]()}
            </SegmentedButtonItem>
          ))}
        </SegmentedButton>
      </SettingsCardContent>
    </SettingsCard>
  )
}
