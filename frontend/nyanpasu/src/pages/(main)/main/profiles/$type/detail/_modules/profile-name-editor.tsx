import { ComponentProps } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { FormDialogError } from '@nyanpasu/ui/dialog'
import { Input } from '@nyanpasu/ui/input'
import { Modal, ModalTrigger, useModal } from '@nyanpasu/ui/modal'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import { useProfileMutations } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import ProfileFieldDialog from './profile-field-dialog'

const formSchema = z.object({
  name: z.string().min(1),
})

export default function ProfileNameEditor(
  props: ComponentProps<typeof ProfileNameEditorForm>,
) {
  return (
    <Modal>
      <ProfileNameEditorForm {...props} />
    </Modal>
  )
}

function ProfileNameEditorForm({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: ProfileItem_Serialize
}) {
  const { patchMetadata } = useProfileMutations()

  const { setOpen } = useModal()

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      name: profile.name,
    },
  })

  const blockTask = useBlockTask(
    `update-profile-name-${profile.uid}`,
    form.handleSubmit(
      async ({ name }) => {
        try {
          await patchMetadata.mutateAsync({
            uid: profile.uid,
            patch: { name },
          })

          setOpen(false)
        } catch (error) {
          message(m.profile_update_failed({ error: formatError(error) }), {
            title: m.profile_error_title(),
            kind: 'error',
            error,
          })
        }
      },
      (error) => {
        console.error(error)
        message(formatError(error.name?.message ?? ''), {
          title: m.profile_error_title(),
          kind: 'error',
        })
      },
    ),
  )

  const handleSubmit = useLockFn(blockTask.execute)

  return (
    <ProfileFieldDialog
      {...props}
      title={m.profile_name_editor_title()}
      onOpen={() => form.reset({ name: profile.name })}
      onSave={handleSubmit}
      saving={blockTask.isPending}
      field={
        <Controller
          control={form.control}
          name="name"
          render={({ field }) => (
            <div className="space-y-2">
              <Input label={m.profile_name_label()} {...field} />

              <FormDialogError message={form.formState.errors.name?.message} />
            </div>
          )}
        />
      }
    />
  )
}
