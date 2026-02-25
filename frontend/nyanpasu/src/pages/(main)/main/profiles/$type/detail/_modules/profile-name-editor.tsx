import { AnimatePresence } from 'framer-motion'
import { ComponentProps, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { Profile, ProfileBuilder, useProfile } from '@nyanpasu/interface'
import AnimatedErrorItem from '../../../_modules/error-item'

const formSchema = z.object({
  name: z.string().min(1),
})

export default function ProfileNameEditor({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: Profile
}) {
  const { patch } = useProfile()

  const [open, setOpen] = useState(false)

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      name: profile.name,
    },
  })

  const handleClose = () => {
    setOpen(false)
    // get latest name
    form.reset({
      name: profile.name,
    })
  }

  const blockTask = useBlockTask(
    `update-profile-name-${profile.uid}`,
    form.handleSubmit(
      async ({ name }) => {
        try {
          await patch.mutateAsync({
            uid: profile.uid,
            profile: {
              ...profile,
              name,
            } as ProfileBuilder,
          })

          handleClose()
        } catch (error) {
          message(`Update failed: \n ${formatError(error)}`, {
            title: 'Error',
            kind: 'error',
          })
        }
      },
      (error) => {
        console.error(error)
        message(formatError(error.name?.message ?? ''), {
          title: 'Error',
          kind: 'error',
        })
      },
    ),
  )

  const handleSubmit = useLockFn(blockTask.execute)

  return (
    <Modal open={open} onOpenChange={setOpen}>
      <ModalTrigger {...props} />

      <ModalContent>
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
            <Button onClick={handleSubmit} loading={blockTask.isPending}>
              {m.common_save()}
            </Button>

            <Button onClick={handleClose}>{m.common_cancel()}</Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}
