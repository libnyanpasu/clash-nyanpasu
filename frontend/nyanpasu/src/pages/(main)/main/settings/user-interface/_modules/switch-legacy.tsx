import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
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
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card'

const currentWindow = getCurrentWebviewWindow()

export default function SwitchLegacy() {
  const { upsert } = useSetting('window_type')

  const handleClick = useLockFn(async () => {
    await upsert('legacy')
    await commands.createLegacyWindow()
    await currentWindow.close()
  })

  return (
    <SettingsCard data-slot="switch-legacy-card">
      <Modal>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>Switch to Legacy UI</ItemLabelText>
                </ItemLabel>

                <div>
                  <ArrowForwardIosRounded />
                </div>
              </ItemContainer>
            </Button>
          </ModalTrigger>
        </SettingsCardContent>

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
    </SettingsCard>
  )
}
