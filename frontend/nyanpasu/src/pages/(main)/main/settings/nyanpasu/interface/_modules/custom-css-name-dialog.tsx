import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input } from '@nyanpasu/ui/input'
import { Modal, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'

/**
 * Asks for the name of a custom CSS item. Mount it to open it: it closes by
 * calling `onClose`, and a fresh mount starts again from `initialName`.
 */
export default function CustomCssNameDialog({
  title,
  initialName,
  onSubmit,
  onClose,
}: {
  title: string
  initialName: string
  onSubmit: (name: string) => Promise<void>
  onClose: () => void
}) {
  const [name, setName] = useState(initialName)

  const trimmedName = name.trim()

  const handleSubmit = async () => {
    if (!trimmedName) {
      return
    }

    await onSubmit(trimmedName)
    onClose()
  }

  return (
    <Modal open onOpenChange={(open) => !open && onClose()}>
      <ModalContent>
        <Card
          className="w-[28rem] max-w-full"
          data-slot="custom-css-name-dialog"
        >
          <CardHeader>
            <ModalTitle>{title}</ModalTitle>
          </CardHeader>

          <CardContent>
            <form
              data-slot="custom-css-name-form"
              onSubmit={(event) => {
                event.preventDefault()
                handleSubmit()
              }}
            >
              <Input
                className="w-full"
                label={m.settings_user_interface_custom_css_name()}
                aria-label={m.settings_user_interface_custom_css_name()}
                value={name}
                autoFocus
                spellCheck={false}
                onChange={(event) => setName(event.target.value)}
              />
            </form>
          </CardContent>

          <CardFooter className="gap-1">
            <Button onClick={onClose}>{m.common_cancel()}</Button>

            <Button
              variant="flat"
              disabled={!trimmedName}
              onClick={handleSubmit}
            >
              {m.common_save()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}
