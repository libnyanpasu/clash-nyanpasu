import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { useCallback, useMemo } from 'react'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { TunStack, useRuntimeProfile, useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

export default function TunStackSelector() {
  const coreType = useSetting('clash_core')

  const tunStack = useSetting('tun_stack')

  const enableTunMode = useSetting('enable_tun_mode')

  const runtimeProfile = useRuntimeProfile()

  const coreTypeValue = coreType.value ?? 'mihomo'

  const tunStackOptions = useMemo(() => {
    const options: {
      [key: string]: string
    } = {
      system: 'System',
      gvisor: 'gVisor',
      mixed: 'Mixed',
    }

    // clash not support mixed
    if (coreTypeValue === 'clash') {
      delete options.mixed
    }
    return options
  }, [coreTypeValue])

  const currentTunStack = useMemo(() => {
    const stack = tunStack.value || 'gvisor'
    return stack in tunStackOptions ? stack : 'gvisor'
  }, [tunStackOptions, tunStack.value])

  const handleTunStackChange = useCallback(
    async (value: string) => {
      try {
        await tunStack.upsert(value as TunStack)

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
        })
      }
    },
    [tunStack, enableTunMode, runtimeProfile],
  )

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
                    {currentTunStack ? tunStackOptions[currentTunStack] : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(tunStackOptions).map(([key, message]) => (
            <DropdownMenuCheckboxItem
              checked={tunStack.value === key}
              key={key}
              onSelect={() => handleTunStackChange(key)}
            >
              {message}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
