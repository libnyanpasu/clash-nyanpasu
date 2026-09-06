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

  const notInstalled = query.data?.status === 'not_installed'

  // fail-closed 兼容门（backend/tauri/src/core/service/compat.rs）：这两态下
  // RunType::classify 永远退回 Normal，开关打开也不会走 Service backend。
  const compatKind = query.data?.compat.kind

  const compatBlocked =
    compatKind === 'incompatible' || compatKind === 'unparsable'

  // 不兼容时只拦「打开」，已开启的仍可关闭——否则用户会被锁死在一个无效的 ON 状态。
  const disabled = notInstalled || (compatBlocked && !serviceMode.value)

  const hint = compatBlocked
    ? m.settings_system_proxy_service_mode_incompatible_tooltip()
    : notInstalled
      ? m.settings_system_proxy_service_mode_disabled_tooltip()
      : null

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

        {hint && (
          <TooltipContent>
            <span>{hint}</span>
          </TooltipContent>
        )}
      </Tooltip>
    </ItemContainer>
  )
}
