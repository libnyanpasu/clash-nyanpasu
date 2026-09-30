import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { rpc } from '@nyanpasu/interface'
import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardHeader,
} from '../../_modules/settings-card'

const currentWindow = isTauri() ? getCurrentWebviewWindow() : null

export default function WindowDebug() {
  const handleCreateEditorWindow = useLockFn(async () => {
    await rpc.createEditorWindow('profile', 'test')
  })

  const handleCreateDebugTrayMenuWindow = useLockFn(async () => {
    await rpc.createDebugTrayMenuWindow()
  })

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>Window Debug Utils</SettingsCardHeader>

        <SettingsCardContent>
          <div className="flex items-center gap-1 select-text">
            <span>Current Window Label:</span>
            <span className="font-mono font-bold">
              {currentWindow?.label ?? 'browser'}
            </span>
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
