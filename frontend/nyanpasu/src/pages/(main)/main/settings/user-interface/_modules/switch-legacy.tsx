import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { useLockFn } from '@/hooks/use-lock-fn'
import { commands, useSetting } from '@nyanpasu/interface'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const currentWindow = getCurrentWebviewWindow()

export default function SwitchLegacy() {
  const { upsert } = useSetting('use_legacy_ui')

  const handleClick = useLockFn(async () => {
    await upsert(true)
    await commands.createLegacyWindow()
    await currentWindow.close()
  })

  return (
    <SettingsCard data-slot="switch-legacy-card">
      <SettingsCardContent
        className="flex items-center justify-between px-2"
        data-slot="switch-legacy-card-content"
      >
        <Card className="w-full space-y-4">
          <CardHeader>Switch to Legacy UI</CardHeader>

          <CardFooter>
            <Modal>
              <ModalTrigger asChild>
                <Button variant="stroked">Open</Button>
              </ModalTrigger>

              <ModalContent>
                <Card className="w-96">
                  <CardHeader>
                    <ModalTitle>
                      Are you sure you want to switch to Legacy UI?
                    </ModalTitle>
                  </CardHeader>

                  <CardContent>
                    <p>
                      Switching to Legacy UI will revert the UI to the original
                      design.
                    </p>
                  </CardContent>

                  <CardFooter className="gap-2">
                    <Button variant="flat" onClick={handleClick}>
                      Continue
                    </Button>

                    <ModalClose asChild>
                      <Button>Cancel</Button>
                    </ModalClose>
                  </CardFooter>
                </Card>
              </ModalContent>
            </Modal>
          </CardFooter>
        </Card>
      </SettingsCardContent>
    </SettingsCard>
  )
}
