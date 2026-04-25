import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { commands } from '@nyanpasu/interface'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardHeader,
} from '../../_modules/settings-card'

const currentWindow = getCurrentWebviewWindow()

export default function WindowDebug() {
  const handleCreateEditorWindow = useLockFn(async () => {
    await commands.createEditorWindow('test')
  })

  const handleCreateDebugTrayMenuWindow = useLockFn(async () => {
    await commands.createDebugTrayMenuWindow()
  })

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>Window Debug Utils</SettingsCardHeader>

        <SettingsCardContent>
          <div className="flex items-center gap-1 select-text">
            <span>Current Window Label:</span>
            <span className="font-mono font-bold">{currentWindow.label}</span>
          </div>

          <div className="flex items-center gap-2">
            <Button variant="flat" onClick={handleCreateEditorWindow}>
              Create Test Editor Window
            </Button>

            <Button variant="flat" onClick={handleCreateDebugTrayMenuWindow}>
              Create Persistent Tray Menu Window
            </Button>
          </div>
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
