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
import { TunStack } from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'

const TUN_STACK_LABELS: Record<TunStack, string> = {
  system: 'System',
  gvisor: 'gVisor',
  mixed: 'Mixed',
  // mihomo's own stack, not the MIPS processor architecture.
  mips: 'Mips (Mihomo IP Stack)',
}

export default function TunStackSelector() {
  const features = useConfigFeatures()

  const tunStack = useClashSetting('tun_stack')

  const enableTunMode = useClashSetting('enable_tun_mode')

  const runtimeProfile = useRuntimeProfile()

  const stacks = features.data?.tun.stack

  const handleTunStackChange = useCallback(
    async (value: TunStack) => {
      try {
        await tunStack.upsert(value)

        if (enableTunMode.value) {
          // just to reload clash config
          await enableTunMode.upsert(true)
        }

        // need manual mutate to refetch runtime profile
        await runtimeProfile.refetch()
      } catch (error) {
        message(`Change Tun Stack failed ! \n Error: ${formatError(error)}`, {
          title: 'Error',
          kind: 'error',
          error,
        })
      }
    },
    [tunStack, enableTunMode, runtimeProfile],
  )

  // A core that does not read the stack has nothing to choose.
  if (stacks?.kind !== 'values') {
    return null
  }

  return (
    <SettingsCard data-slot="tun-stack-selector-card">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent data-slot="tun-stack-selector-trigger" asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_tun_stack_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {tunStack.value ? TUN_STACK_LABELS[tunStack.value] : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {stacks.value.map((stack) => (
            <DropdownMenuCheckboxItem
              checked={tunStack.value === stack}
              key={stack}
              onSelect={() => handleTunStackChange(stack)}
            >
              {TUN_STACK_LABELS[stack]}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
