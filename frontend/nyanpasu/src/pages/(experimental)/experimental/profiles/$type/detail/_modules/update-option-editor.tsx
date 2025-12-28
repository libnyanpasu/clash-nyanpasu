import { AnimatePresence } from 'framer-motion'
import { ComponentProps, useCallback, useEffect, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { Input, NumericInput } from '@/components/ui/input'
import { Modal, ModalContent, ModalTrigger } from '@/components/ui/modal'
import { Switch } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { ProfileBuilder, RemoteProfile, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import AnimatedErrorItem from '../../../_modules/error-item'

const formSchema = z.object({
  user_agent: z.string().optional(),
  with_proxy: z.boolean().optional(),
  self_proxy: z.boolean().optional(),
  update_interval: z.number().optional(),
})

const SwitchItem = ({
  label,
  ...props
}: ComponentProps<typeof Switch> & {
  label: string
}) => {
  return (
    <div
      className={cn(
        'flex h-16 w-full items-center justify-between gap-2',
        'bg-surface-variant/30 dark:bg-surface-variant/10',
        'rounded-xl',
        'p-4',
      )}
    >
      <div>{label}</div>

      <Switch {...props} />
    </div>
  )
}

export default function UpdateOptionEditor({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: RemoteProfile
}) {
  const { patch } = useProfile()

  const [open, setOpen] = useState(false)

  const getDefaultValues = useCallback(() => {
    return {
      user_agent: profile.option?.user_agent ?? '',
      with_proxy: profile.option?.with_proxy ?? false,
      self_proxy: profile.option?.self_proxy ?? false,
      update_interval: profile.option?.update_interval ?? 0,
    }
  }, [profile.option])

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: getDefaultValues(),
  })

  // sync profile option to form
  useEffect(() => {
    form.reset(getDefaultValues())
  }, [form, getDefaultValues])

  const handleClose = () => {
    setOpen(false)
    form.reset(getDefaultValues())
  }

  const blockTask = useBlockTask(
    `update-remote-profile-${profile.uid}`,
    form.handleSubmit(
      async (data) => {
        try {
          await patch.mutateAsync({
            uid: profile.uid,
            profile: {
              ...profile,
              option: {
                ...profile.option,
                ...data,
              },
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
        message(formatError(error), {
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
          <CardHeader>{m.profile_update_option_editor_title()}</CardHeader>

          <CardContent>
            <Controller
              control={form.control}
              name="user_agent"
              render={({ field }) => (
                <div className="mt-2 flex items-center gap-2">
                  <Input
                    label={m.profile_user_agent_label()}
                    variant="outlined"
                    {...field}
                  />

                  <AnimatePresence>
                    {form.formState.errors.user_agent && (
                      <AnimatedErrorItem className="text-error">
                        {form.formState.errors.user_agent.message}
                      </AnimatedErrorItem>
                    )}
                  </AnimatePresence>
                </div>
              )}
            />

            <Controller
              control={form.control}
              name="update_interval"
              render={({ field }) => (
                <div className="mt-2 flex items-center gap-2">
                  <NumericInput
                    label={m.profile_update_interval_label()}
                    variant="outlined"
                    min={0}
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

            <Controller
              control={form.control}
              name="with_proxy"
              render={({ field }) => (
                <SwitchItem
                  label={m.profile_with_proxy_label()}
                  checked={field.value}
                  onCheckedChange={(checked) => field.onChange(checked)}
                />
              )}
            />

            <Controller
              control={form.control}
              name="self_proxy"
              render={({ field }) => (
                <SwitchItem
                  label={m.profile_self_proxy_label()}
                  checked={field.value}
                  onCheckedChange={(checked) => field.onChange(checked)}
                />
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
