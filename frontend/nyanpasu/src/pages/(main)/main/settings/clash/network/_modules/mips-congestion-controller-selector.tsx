import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { useCallback } from 'react'
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
import {
  useClashSetting,
  useConfigFeatures,
  useRuntimeProfile,
} from '@nyanpasu/query'
import { TunCongestionController } from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'

const CONGESTION_CONTROLLER_LABELS: Record<TunCongestionController, string> = {
  cubic: 'CUBIC',
  reno: 'Reno',
  bbr: 'BBR',
  bbr3: 'BBRv3',
}

/** The TCP congestion controller of the mips stack, shown while it is selected. */
export default function MipsCongestionControllerSelector() {
  const features = useConfigFeatures()

  const tunStack = useClashSetting('tun_stack')

  const tunMips = useClashSetting('tun_mips')

  const runtimeProfile = useRuntimeProfile()

  const controllers = features.data?.tun.congestion_controller

  const current = tunMips.value?.congestion_controller ?? null

  const handleChange = useCallback(
    async (value: TunCongestionController | null) => {
      try {
        // `null` hands the controller back to the core's default.
        await tunMips.upsert({ congestion_controller: value })

        await runtimeProfile.refetch()
      } catch (error) {
        message(formatError(error), { title: 'Error', kind: 'error', error })
      }
    },
    [tunMips, runtimeProfile],
  )

  if (tunStack.value !== 'mips' || controllers?.kind !== 'values') {
    return null
  }

  return (
    <SettingsCard data-slot="mips-congestion-controller-selector-card">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="mips-congestion-controller-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_tun_congestion_controller_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {current
                      ? CONGESTION_CONTROLLER_LABELS[current]
                      : m.settings_clash_settings_tun_congestion_controller_core_default()}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          <DropdownMenuCheckboxItem
            checked={current === null}
            onSelect={() => handleChange(null)}
          >
            {m.settings_clash_settings_tun_congestion_controller_core_default()}
          </DropdownMenuCheckboxItem>

          {controllers.value.map((controller) => (
            <DropdownMenuCheckboxItem
              checked={current === controller}
              key={controller}
              onSelect={() => handleChange(controller)}
            >
              {CONGESTION_CONTROLLER_LABELS[controller]}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
