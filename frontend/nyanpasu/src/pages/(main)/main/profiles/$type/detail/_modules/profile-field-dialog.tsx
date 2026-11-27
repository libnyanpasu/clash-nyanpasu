import { ComponentProps, ReactNode } from 'react'
import { DialogTrigger, FormDialogContent } from '@nyanpasu/ui/dialog'
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
}: ComponentProps<typeof DialogTrigger> & {
  title: ReactNode
  /** The input and its validation message. */
  field: ReactNode
  onOpen: () => void
  onSave: () => void | Promise<void>
  saving: boolean
}) {
  return (
    <>
      <DialogTrigger {...props} />

      <FormDialogContent
        title={title}
        submitLabel={m.common_save()}
        cancelLabel={m.common_cancel()}
        submitting={saving}
        onOpen={onOpen}
        onSubmit={onSave}
      >
        <div data-slot="profile-field-dialog" className="flex flex-col gap-4">
          {field}
        </div>
      </FormDialogContent>
    </>
  )
}
