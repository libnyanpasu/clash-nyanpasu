import { Switch } from '@nyanpasu/ui/switch'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import ServiceFallbackWarning from '@/components/settings/service-fallback-warning'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting, useSystemService } from '@nyanpasu/query'
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

  // 未安装或不兼容时只拦「打开」，已开启的仍可关闭；开启期间内核回退到本地运行，
  // 原因由开关旁的警告图标说明。
  const disabled = !serviceMode.value && (notInstalled || compatBlocked)

  const hint = !disabled
    ? null
    : compatBlocked
      ? m.settings_system_proxy_service_mode_incompatible_tooltip()
      : m.settings_system_proxy_service_mode_disabled_tooltip()

  const handleServiceMode = useLockFn(async () => {
    try {
      await serviceMode.upsert(!serviceMode.value)
    } catch (error) {
      message(
        `Activation Service Mode failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
          error,
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

      <div
        className="flex shrink-0 items-center gap-2"
        data-slot="system-service-switch-control"
      >
        <ServiceFallbackWarning />

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
      </div>
    </ItemContainer>
  )
}
