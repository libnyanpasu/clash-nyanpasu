import { AnimatePresence } from 'motion/react'
import { ComponentProps } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@nyanpasu/ui/modal'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import { useProfileMutations } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import AnimatedErrorItem from '../../../../_modules/error-item'

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
    <>
      <ModalTrigger {...props} />

      <ModalContent
        dismissible={!blockTask.isPending}
        onOpenAutoFocus={() => form.reset({ name: profile.name })}
      >
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>{m.profile_name_editor_title()}</ModalTitle>
          </CardHeader>

          <CardContent>
            <Controller
              control={form.control}
              name="name"
              render={({ field }) => (
                <div className="space-y-2">
                  <Input
                    label={m.profile_name_label()}
                    variant="outlined"
                    {...field}
                  />

                  <AnimatePresence>
                    {form.formState.errors.name && (
                      <AnimatedErrorItem className="text-error">
                        {form.formState.errors.name?.message}
                      </AnimatedErrorItem>
                    )}
                  </AnimatePresence>
                </div>
              )}
            />
          </CardContent>

          <CardFooter className="gap-1">
            <Button
              onClick={handleSubmit}
              loading={blockTask.isPending}
              disabled={blockTask.isPending}
            >
              {m.common_save()}
            </Button>

            <ModalClose disabled={blockTask.isPending}>
              {m.common_cancel()}
            </ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </>
  )
}
