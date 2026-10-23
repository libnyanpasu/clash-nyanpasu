import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalContent,
  ModalDescription,
  ModalTitle,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'

/** Asks before navigating away from a page with unapplied changes. */
export default function LeaveDialog({
  open,
  onStay,
  onLeave,
}: {
  open: boolean
  onStay: () => void
  onLeave: () => void
}) {
  return (
    <Modal open={open} onOpenChange={(next) => !next && onStay()}>
      <ModalContent>
        <Card className="w-96" data-slot="profile-global-leave-dialog">
          <CardHeader>
            <ModalTitle>{m.profile_global_leave_title()}</ModalTitle>
          </CardHeader>

          <CardContent>
            <ModalDescription className="text-on-surface-variant text-sm">
              {m.profile_global_leave_description()}
            </ModalDescription>
          </CardContent>

          <CardFooter className="gap-1">
            <Button onClick={onStay}>{m.profile_global_leave_stay()}</Button>

            <Button variant="flat" onClick={onLeave}>
              {m.profile_global_leave_confirm()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}
