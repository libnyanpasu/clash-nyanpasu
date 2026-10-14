import { AnimatePresence } from 'motion/react'
import { ComponentProps, useCallback, useEffect, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { AnimatedItem } from '@nyanpasu/ui/animated-item'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input, NumericInput } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { SwitchItem } from '@nyanpasu/ui/switch'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import { getRemoteSource, useProfileMutations } from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import AnimatedErrorItem from '../../../_modules/error-item'

// Mirrors the backend default; saved when auto update is on and the interval
// is left empty.
// TODO: prefer the subscription's suggested interval (`profile-update-interval`)
// once the profile stores it; today it is applied only at import.
const DEFAULT_UPDATE_INTERVAL_MINUTES = 120

// The backend turns auto update off with an interval of 0 minutes.
const AUTO_UPDATE_OFF_MINUTES = 0

const formSchema = z.object({
  user_agent: z.string().optional(),
  with_proxy: z.boolean().optional(),
  self_proxy: z.boolean().optional(),
  auto_update: z.boolean(),
  update_interval: z
    .number()
    .min(1, {
      message: m.profile_form_option_update_interval_min_error(),
    })
    .nullable()
    .optional(),
})

const remoteOptionOf = (profile: ProfileItem_Serialize) =>
  getRemoteSource(profile)?.option

export default function UpdateOptionEditor({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: ProfileItem_Serialize
}) {
  const { patchRemoteOptions } = useProfileMutations()

  const [open, setOpen] = useState(false)

  const getDefaultValues = useCallback(() => {
    const option = remoteOptionOf(profile)
    const minutes = option?.update_interval_minutes
    return {
      user_agent: option?.user_agent ?? '',
      with_proxy: option?.with_proxy ?? false,
      self_proxy: option?.self_proxy ?? false,
      auto_update: minutes !== AUTO_UPDATE_OFF_MINUTES,
      update_interval:
        minutes != null && minutes > AUTO_UPDATE_OFF_MINUTES ? minutes : null,
    }
  }, [profile])

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: getDefaultValues(),
  })

  const autoUpdate = form.watch('auto_update')

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
          await patchRemoteOptions.mutateAsync({
            uid: profile.uid,
            patch: {
              user_agent: data.user_agent || null,
              with_proxy: data.with_proxy ?? null,
              self_proxy: data.self_proxy ?? null,
              update_interval_minutes: data.auto_update
                ? (data.update_interval ?? DEFAULT_UPDATE_INTERVAL_MINUTES)
                : AUTO_UPDATE_OFF_MINUTES,
            },
          })
          handleClose()
        } catch (error) {
          message(`Update failed: \n ${formatError(error)}`, {
            title: 'Error',
            kind: 'error',
            error,
          })
        }
      },
      (error) => {
        console.error(error)
        message(formatError(error), {
          title: 'Error',
          kind: 'error',
          error,
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
            <ModalTitle>{m.profile_update_option_editor_title()}</ModalTitle>
          </CardHeader>

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
              name="auto_update"
              render={({ field }) => (
                <SwitchItem
                  className="mt-2"
                  checked={field.value}
                  onCheckedChange={(checked) => field.onChange(checked)}
                >
                  <span>{m.profile_update_option_auto_update_label()}</span>
                </SwitchItem>
              )}
            />

            <AnimatePresence initial={false}>
              {autoUpdate && (
                <AnimatedItem key="update-interval">
                  <Controller
                    control={form.control}
                    name="update_interval"
                    render={({ field }) => (
                      <div className="mt-2 space-y-2">
                        <NumericInput
                          label={m.profile_update_interval_label()}
                          variant="outlined"
                          min={1}
                          step={1}
                          {...field}
                        />

                        <p className="text-on-surface-variant px-1 text-xs">
                          {m.profile_update_option_interval_hint({
                            minutes: DEFAULT_UPDATE_INTERVAL_MINUTES,
                          })}
                        </p>

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
                </AnimatedItem>
              )}
            </AnimatePresence>

            <Controller
              control={form.control}
              name="with_proxy"
              render={({ field }) => (
                <SwitchItem
                  checked={field.value}
                  onCheckedChange={(checked) => field.onChange(checked)}
                >
                  <span>{m.profile_with_proxy_label()}</span>
                </SwitchItem>
              )}
            />

            <Controller
              control={form.control}
              name="self_proxy"
              render={({ field }) => (
                <SwitchItem
                  checked={field.value}
                  onCheckedChange={(checked) => field.onChange(checked)}
                >
                  <span>{m.profile_self_proxy_label()}</span>
                </SwitchItem>
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
