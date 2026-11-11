import { ComponentProps, ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  ModalClose,
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
  onOpen,
  onSave,
  saving,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  title: ReactNode
  /** The input and its validation message. */
  field: ReactNode
  onOpen: () => void
  onSave: () => void
  saving: boolean
}) {
  return (
    <>
      <ModalTrigger {...props} />

      <ModalContent dismissible={!saving} onOpenAutoFocus={onOpen}>
        <Card className="w-96" data-slot="profile-field-dialog">
          <CardHeader>
            <ModalTitle>{title}</ModalTitle>
          </CardHeader>

          <CardContent>{field}</CardContent>

          <CardFooter className="gap-1">
            <Button onClick={onSave} loading={saving} disabled={saving}>
              {m.common_save()}
            </Button>

            <ModalClose disabled={saving}>{m.common_cancel()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </>
  )
}
