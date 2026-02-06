import { Switch } from '@/components/ui/switch'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSetting, useSystemService } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function SystemServiceSwitch() {
  const serviceMode = useSetting('enable_service_mode')

  const { query } = useSystemService()

  const disabled = query.data?.status === 'not_installed'

  const handleServiceMode = useLockFn(async () => {
    try {
      await serviceMode.upsert(!serviceMode.value)
    } catch (error) {
      message(
        `Activation Service Mode failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      )
    }
  })

  return (
    <SettingsCard data-slot="system-service-switch-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="system-service-switch-card-content"
      >
        <div>{m.settings_system_proxy_service_mode_label()}</div>

        <Tooltip>
          <TooltipTrigger asChild>
            <div>
              <Switch
                checked={Boolean(serviceMode.value)}
                onCheckedChange={handleServiceMode}
                loading={serviceMode.isPending}
                disabled={disabled}
              />
            </div>
          </TooltipTrigger>

          {disabled && (
            <TooltipContent>
              <span>
                {m.settings_system_proxy_service_mode_disabled_tooltip()}
              </span>
            </TooltipContent>
          )}
        </Tooltip>
      </SettingsCardContent>
    </SettingsCard>
  )
}
