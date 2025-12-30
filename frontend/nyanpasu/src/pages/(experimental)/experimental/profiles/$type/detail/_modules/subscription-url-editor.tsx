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
import { ProfileBuilder, RemoteProfile, useProfile } from '@nyanpasu/interface'
import AnimatedErrorItem from '../../../_modules/error-item'

const formSchema = z.object({
  url: z.httpUrl(),
})

export default function SubscriptionUrlEditor({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: RemoteProfile
}) {
  const { patch } = useProfile()

  const [open, setOpen] = useState(false)

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      url: profile.url,
    },
  })

  const handleClose = () => {
    setOpen(false)
    // get latest name
    form.reset({
      url: profile.url,
    })
  }

  const blockTask = useBlockTask(
    `update-remote-profile-url-${profile.uid}`,
    form.handleSubmit(
      async ({ url }) => {
        try {
          await patch.mutateAsync({
            uid: profile.uid,
            profile: {
              ...profile,
              url,
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
        message(formatError(error.url?.message ?? ''), {
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
            <ModalTitle>{m.profile_subscription_url_editor_label()}</ModalTitle>
          </CardHeader>

          <CardContent>
            <Controller
              control={form.control}
              name="url"
              render={({ field }) => (
                <div className="space-y-2">
                  <Input
                    label={m.profile_subscription_url_label()}
                    variant="outlined"
                    {...field}
                  />

                  <AnimatePresence>
                    {form.formState.errors.url && (
                      <AnimatedErrorItem className="text-error">
                        {form.formState.errors.url?.message}
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
