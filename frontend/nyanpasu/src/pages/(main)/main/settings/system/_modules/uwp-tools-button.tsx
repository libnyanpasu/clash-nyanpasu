import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { commands } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function UwpToolsButton() {
  const handleOpenUwpTools = useLockFn(async () => {
    await commands.invokeUwpTool()
  })

  return (
    <SettingsCard data-slot="uwp-tools-button-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="uwp-tools-button-card-content"
      >
        <div>{m.settings_system_proxy_uwp_tools_label()}</div>

        <Button variant="flat" onClick={handleOpenUwpTools}>
          {m.common_open()}
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}
