import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { useLockFn } from '@/hooks/use-lock-fn'
import { commands } from '@nyanpasu/interface'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const currentWindow = getCurrentWebviewWindow()

export default function WindowDebug() {
  const handleCreateLegacyWindow = useLockFn(async () => {
    await commands.createLegacyWindow()
  })

  const handleCreateEditorWindow = useLockFn(async () => {
    await commands.createEditorWindow('test')
  })

  return (
    <SettingsCard data-slot="window-debug-card">
      <SettingsCardContent
        data-slot="window-debug-card-content"
        className="flex flex-col gap-3 px-2"
      >
        <Card>
          <CardHeader>Window Debug Utils</CardHeader>

          <CardContent>
            <div className="flex items-center gap-1 select-text">
              <span>Current Window Label:</span>
              <span className="font-mono font-bold">{currentWindow.label}</span>
            </div>

            <div className="flex items-center gap-2">
              <Button variant="flat" onClick={handleCreateLegacyWindow}>
                Create Legacy Window
              </Button>

              <Button variant="flat" onClick={handleCreateEditorWindow}>
                Create Test Editor Window
              </Button>
            </div>
          </CardContent>
        </Card>
      </SettingsCardContent>
    </SettingsCard>
  )
}
