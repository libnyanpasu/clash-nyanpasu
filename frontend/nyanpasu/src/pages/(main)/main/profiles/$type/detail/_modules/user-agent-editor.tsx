import { ComponentProps } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { Input } from '@nyanpasu/ui/input'
import { Modal, ModalTrigger, useModal } from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { getRemoteSource } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import ProfileFieldDialog from './profile-field-dialog'
import {
  UNCHANGED_REMOTE_OPTIONS,
  useRemoteOptionsPatch,
} from './use-remote-options-patch'

/** Edits the User-Agent a remote profile is downloaded with. */
export default function UserAgentEditor(
  props: ComponentProps<typeof UserAgentEditorForm>,
) {
  return (
    <Modal>
      <UserAgentEditorForm {...props} />
    </Modal>
  )
}

function UserAgentEditorForm({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: ProfileItem_Serialize
}) {
  const { patch, isPending } = useRemoteOptionsPatch(profile.uid, 'user-agent')

  const { setOpen } = useModal()

  const currentUserAgent = getRemoteSource(profile)?.option.user_agent ?? ''

  const form = useForm({ defaultValues: { user_agent: currentUserAgent } })

  const handleSave = form.handleSubmit(async ({ user_agent }) => {
    // An empty value clears the override, so the default applies again.
    const saved = await patch({
      ...UNCHANGED_REMOTE_OPTIONS,
      user_agent: user_agent.trim() || null,
    })

    if (saved) {
      setOpen(false)
    }
  })

  return (
    <ProfileFieldDialog
      {...props}
      title={m.profile_user_agent_editor_label()}
      onOpen={() => form.reset({ user_agent: currentUserAgent })}
      onSave={handleSave}
      saving={isPending}
      field={
        <Controller
          control={form.control}
          name="user_agent"
          render={({ field }) => (
            <div className="space-y-2">
              <Input
                label={m.profile_user_agent_label()}
                variant="outlined"
                {...field}
              />

              <p className="text-on-surface-variant px-1 text-xs">
                {m.profile_user_agent_editor_hint()}
              </p>
            </div>
          )}
        />
      }
    />
  )
}
