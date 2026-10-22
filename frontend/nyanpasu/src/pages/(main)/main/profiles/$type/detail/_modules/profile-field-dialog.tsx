import { ComponentProps, ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'

/**
 * The shell of a dialog that edits one field of a profile: the trigger it
 * wraps, a title, the field, and the save and cancel buttons.
 */
export default function ProfileFieldDialog({
  title,
  field,
  open,
  onOpenChange,
  onSave,
  onCancel,
  saving,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  title: ReactNode
  /** The input and its validation message. */
  field: ReactNode
  open: boolean
  onOpenChange: (open: boolean) => void
  onSave: () => void
  onCancel: () => void
  saving: boolean
}) {
  return (
    <Modal open={open} onOpenChange={onOpenChange}>
      <ModalTrigger {...props} />

      <ModalContent>
        <Card className="w-96" data-slot="profile-field-dialog">
          <CardHeader>
            <ModalTitle>{title}</ModalTitle>
          </CardHeader>

          <CardContent>{field}</CardContent>

          <CardFooter className="gap-1">
            <Button onClick={onSave} loading={saving}>
              {m.common_save()}
            </Button>

            <Button onClick={onCancel}>{m.common_cancel()}</Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}
