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
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../_modules/settings-card'

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
    <ItemContainer data-slot="system-service-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_service_mode_label()}
        </ItemLabelText>

        <ItemLabelDescription>
          {m.settings_system_proxy_service_mode_description()}
        </ItemLabelDescription>
      </ItemLabel>

      <Tooltip>
        <TooltipTrigger asChild>
          <div data-slot="system-service-switch-trigger-wrapper">
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
    </ItemContainer>
  )
}
