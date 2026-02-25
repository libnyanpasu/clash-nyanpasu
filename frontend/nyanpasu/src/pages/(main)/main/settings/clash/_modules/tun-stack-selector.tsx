import { useCallback, useMemo } from 'react'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useCoreType } from '@/hooks/use-store'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { TunStack, useRuntimeProfile, useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function TunStackSelector() {
  const [coreType] = useCoreType()

  const tunStack = useSetting('tun_stack')

  const enableTunMode = useSetting('enable_tun_mode')

  const runtimeProfile = useRuntimeProfile()

  const tunStackOptions = useMemo(() => {
    const options: {
      [key: string]: string
    } = {
      system: 'System',
      gvisor: 'gVisor',
      mixed: 'Mixed',
    }

    // clash not support mixed
    if (coreType === 'clash') {
      delete options.mixed
    }
    return options
  }, [coreType])

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
      <SettingsCardContent
        className="px-2"
        data-slot="tun-stack-selector-card-content"
      >
        <Select
          variant="outlined"
          value={currentTunStack}
          onValueChange={handleTunStackChange}
        >
          <SelectTrigger>
            <SelectValue
              placeholder={m.settings_clash_settings_tun_stack_label()}
            >
              {tunStackOptions[currentTunStack]}
            </SelectValue>
          </SelectTrigger>

          <SelectContent>
            <SelectGroup>
              {Object.entries(tunStackOptions).map(([key, value]) => (
                <SelectItem key={key} value={key}>
                  {value}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      </SettingsCardContent>
    </SettingsCard>
  )
}
