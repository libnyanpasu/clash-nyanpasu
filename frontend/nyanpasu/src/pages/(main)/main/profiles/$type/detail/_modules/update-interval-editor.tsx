import { AnimatePresence } from 'motion/react'
import { ComponentProps } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { NumericInput } from '@nyanpasu/ui/input'
import { Modal, ModalTrigger, useModal } from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { zodResolver } from '@hookform/resolvers/zod'
import { getRemoteSource } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import AnimatedErrorItem from '../../../../_modules/error-item'
import ProfileFieldDialog from './profile-field-dialog'
import {
  UNCHANGED_REMOTE_OPTIONS,
  useRemoteOptionsPatch,
} from './use-remote-options-patch'

const formSchema = z.object({
  update_interval: z
    .number({ error: () => m.profile_form_option_update_interval_min_error() })
    .min(1, { error: () => m.profile_form_option_update_interval_min_error() }),
})

/** Edits the minutes between the automatic updates of a remote profile. */
export default function UpdateIntervalEditor(
  props: ComponentProps<typeof UpdateIntervalEditorForm>,
) {
  return (
    <Modal>
      <UpdateIntervalEditorForm {...props} />
    </Modal>
  )
}

function UpdateIntervalEditorForm({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: ProfileItem_Serialize
}) {
  const { patch, isPending } = useRemoteOptionsPatch(
    profile.uid,
    'update-interval',
  )

  const { setOpen } = useModal()

  const currentInterval =
    getRemoteSource(profile)?.option.update_interval_minutes ?? 0

  const form = useForm<z.input<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: { update_interval: currentInterval },
  })

  const handleSave = form.handleSubmit(async ({ update_interval }) => {
    const saved = await patch({
      ...UNCHANGED_REMOTE_OPTIONS,
      update_interval_minutes: update_interval,
    })

    if (saved) {
      setOpen(false)
    }
  })

  return (
    <ProfileFieldDialog
      {...props}
      title={m.profile_update_interval_editor_label()}
      onOpen={() => form.reset({ update_interval: currentInterval })}
      onSave={handleSave}
      saving={isPending}
      field={
        <Controller
          control={form.control}
          name="update_interval"
          render={({ field }) => (
            <div className="space-y-2">
              <NumericInput
                label={m.profile_update_interval_label()}
                variant="outlined"
                min={1}
                step={1}
                {...field}
              />

              <AnimatePresence>
                {form.formState.errors.update_interval && (
                  <AnimatedErrorItem className="text-error">
                    {form.formState.errors.update_interval.message}
                  </AnimatedErrorItem>
                )}
              </AnimatePresence>
            </div>
          )}
        />
      }
    />
  )
}
