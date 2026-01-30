import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import { AnimatePresence } from 'framer-motion'
import { PropsWithChildren, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import z from 'zod'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { Input, NumericInput } from '@/components/ui/input'
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { ScrollArea } from '@/components/ui/scroll-area'
import { SwitchItem } from '@/components/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { RemoteProfileBuilder, useProfile } from '@nyanpasu/interface'
import AnimatedErrorItem from '../../_modules/error-item'

const formSchema = z.object({
  type: z.literal('remote'),
  uid: z.string().nullable(),
  name: z.string().nullable(),
  file: z.string().nullable(),
  desc: z.string().nullable(),
  updated: z.number().nullable(),
  url: z.httpUrl(),
  extra: z
    .object({
      upload: z.number(),
      download: z.number(),
      total: z.number(),
      expire: z.number(),
    })
    .nullable(),
  option: z
    .object({
      user_agent: z.string().nullable(),
      with_proxy: z.boolean(),
      self_proxy: z.boolean(),
      update_interval: z.number().nullable(),
    })
    .optional(),
  chain: z.array(z.string()).nullable().optional(),
}) satisfies z.ZodType<RemoteProfileBuilder>

const getDefaultValues = () => {
  return {
    type: 'remote',
    uid: null,
    name: `${m.profile_import_remote_title()} - ${dayjs().format('YYYY-MM-DD HH:mm:ss')}`,
    file: null,
    desc: null,
    updated: null,
    url: '',
    extra: null,
    option: {
      with_proxy: false,
      self_proxy: false,
      update_interval: 1440,
      user_agent: null,
    },
    chain: null,
  } satisfies z.infer<typeof formSchema>
}

export default function RemoteProfileButton({ children }: PropsWithChildren) {
  const { create } = useProfile()

  const [open, setOpen] = useState(false)

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: getDefaultValues(),
  })

  const blockTask = useBlockTask(
    `create-remote-profile`,
    form.handleSubmit(async (data) => {
      try {
        await create.mutateAsync({
          type: 'manual',
          data: {
            item: data,
            fileData: null,
          },
        })

        handleToggle(false)
      } catch (error) {
        message(`Create failed: \n ${formatError(error)}`, {
          title: 'Error',
          kind: 'error',
        })
      }
    }),
  )

  const handleToggle = (value: boolean) => {
    if (blockTask.isPending) {
      return
    }

    setOpen(value)

    if (value) {
      form.reset(getDefaultValues())
    }
  }

  const handleSubmit = useLockFn(blockTask.execute)

  return (
    <Modal open={open} onOpenChange={handleToggle}>
      <ModalTrigger asChild>{children}</ModalTrigger>

      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>{m.profile_import_remote_title()}</ModalTitle>
          </CardHeader>

          <CardContent asChild>
            <ScrollArea className="max-h-[calc(100vh-200px)]">
              <div className="space-y-4 pt-2">
                <Controller
                  control={form.control}
                  name="name"
                  render={({ field }) => (
                    <div className="space-y-2">
                      <Input
                        variant="outlined"
                        label={m.profile_form_name_label()}
                        {...field}
                        value={field.value ?? ''}
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

                <Controller
                  control={form.control}
                  name="desc"
                  render={({ field }) => (
                    <div className="space-y-2">
                      <Input
                        variant="outlined"
                        label={m.profile_form_desc_label()}
                        {...field}
                        value={field.value ?? ''}
                      />

                      <AnimatePresence>
                        {form.formState.errors.desc && (
                          <AnimatedErrorItem className="text-error">
                            {form.formState.errors.desc?.message}
                          </AnimatedErrorItem>
                        )}
                      </AnimatePresence>
                    </div>
                  )}
                />

                <Controller
                  control={form.control}
                  name="url"
                  render={({ field }) => (
                    <div className="space-y-2">
                      <Input
                        variant="outlined"
                        label={m.profile_form_url_label()}
                        {...field}
                        value={field.value ?? ''}
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

                <Controller
                  control={form.control}
                  name="option.user_agent"
                  render={({ field }) => (
                    <div className="space-y-2">
                      <Input
                        variant="outlined"
                        label={m.profile_form_option_user_agent_label()}
                        {...field}
                        value={field.value ?? ''}
                      />

                      <AnimatePresence>
                        {form.formState.errors.option?.user_agent && (
                          <AnimatedErrorItem className="text-error">
                            {form.formState.errors.option?.user_agent?.message}
                          </AnimatedErrorItem>
                        )}
                      </AnimatePresence>
                    </div>
                  )}
                />

                <Controller
                  control={form.control}
                  name="option.update_interval"
                  render={({ field }) => (
                    <div className="space-y-2">
                      <NumericInput
                        variant="outlined"
                        label={m.profile_form_option_update_interval_label()}
                        min={0}
                        step={1}
                        {...field}
                      />

                      <AnimatePresence>
                        {form.formState.errors.option?.update_interval && (
                          <AnimatedErrorItem className="text-error">
                            {
                              form.formState.errors.option?.update_interval
                                .message
                            }
                          </AnimatedErrorItem>
                        )}
                      </AnimatePresence>
                    </div>
                  )}
                />

                <Controller
                  control={form.control}
                  name="option.with_proxy"
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
                  name="option.self_proxy"
                  render={({ field }) => (
                    <SwitchItem
                      checked={field.value}
                      onCheckedChange={(checked) => field.onChange(checked)}
                    >
                      <span>{m.profile_self_proxy_label()}</span>
                    </SwitchItem>
                  )}
                />
              </div>
            </ScrollArea>
          </CardContent>

          <CardFooter className="gap-1">
            <Button onClick={handleSubmit} loading={blockTask.isPending}>
              {m.common_submit()}
            </Button>

            <Button onClick={() => handleToggle(false)}>
              {m.common_cancel()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}
