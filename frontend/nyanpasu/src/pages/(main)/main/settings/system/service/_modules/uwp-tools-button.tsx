import { ItemAction } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { useLockFn } from '@nyanpasu/hooks'

export default function UwpToolsButton() {
  const handleOpenUwpTools = useLockFn(async () => {
    await rpc.invokeUwpTool()
  })

  return (
    <ItemAction
      data-slot="uwp-tools-button-card"
      title={m.settings_system_proxy_uwp_tools_label()}
      description={m.settings_system_proxy_uwp_tools_description()}
      onClick={handleOpenUwpTools}
    />
  )
}
