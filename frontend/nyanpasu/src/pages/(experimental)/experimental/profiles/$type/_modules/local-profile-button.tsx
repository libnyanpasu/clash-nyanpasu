import UploadFileRounded from '~icons/material-symbols/upload-file-rounded'
import dayjs from 'dayjs'
import { AnimatePresence } from 'framer-motion'
import { PropsWithChildren, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import z from 'zod'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import {
  FileDropZone,
  FileDropZoneFileSelected,
  FileDropZoneLoading,
  FileDropZonePlaceholder,
} from '@/components/ui/file-drop-zone'
import { Input } from '@/components/ui/input'
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { CircularProgress } from '@/components/ui/progress'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import parseTraffic from '@/utils/parse-traffic'
import { zodResolver } from '@hookform/resolvers/zod'
import { LocalProfileBuilder, useProfile } from '@nyanpasu/interface'
import AnimatedErrorItem from '../../_modules/error-item'

const formSchema = z.object({
  uid: z.string().nullable(),
  name: z.string().nullable(),
  file: z.string().nullable(),
  desc: z.string().nullable(),
  updated: z.number().nullable(),
  symlinks: z.string().nullable(),
  chain: z.array(z.string()).nullable().optional(),
}) satisfies z.ZodType<LocalProfileBuilder>

const acceptFiles = ['.yaml', '.yml']

const getDefaultValues = () => {
  return {
    uid: null,
    name: `${m.profile_import_local_title()} - ${dayjs().format('YYYY-MM-DD HH:mm:ss')}`,
    file: null,
    desc: null,
    updated: null,
    symlinks: null,
    chain: null,
  } satisfies z.infer<typeof formSchema>
}

export default function LocalProfileButton({ children }: PropsWithChildren) {
  const { create } = useProfile()

  const [open, setOpen] = useState(false)

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: getDefaultValues(),
  })

  const blockTask = useBlockTask(
    `create-local-profile`,
    form.handleSubmit(async (data) => {
      try {
        await create.mutateAsync({
          type: 'manual',
          data: {
            item: {
              type: 'local',
              ...data,
            },
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
            <ModalTitle>{m.profile_import_local_title()}</ModalTitle>
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
                  name="file"
                  render={({ field }) => (
                    <FileDropZone
                      accept={acceptFiles}
                      value={field.value}
                      onChange={(name) => {
                        form.setValue('desc', name)
                      }}
                      onFileRead={(value) => field.onChange(value)}
                      disabled={blockTask.isPending}
                    >
                      <FileDropZonePlaceholder className="flex flex-col items-center justify-center gap-2">
                        <UploadFileRounded className="text-on-surface-variant size-8" />

                        <span className="text-on-surface-variant text-sm">
                          {m.profile_import_local_file_placeholder()}
                        </span>

                        <span className="text-on-surface-variant text-xs">
                          {m.profile_import_local_file_type_label({
                            types: acceptFiles.join(', '),
                          })}
                        </span>
                      </FileDropZonePlaceholder>

                      <FileDropZoneLoading>
                        <CircularProgress className="size-8" indeterminate />
                      </FileDropZoneLoading>

                      <FileDropZoneFileSelected className="flex flex-col items-center justify-center gap-2">
                        <UploadFileRounded className="text-primary size-8" />

                        <div className="text-on-surface max-w-full truncate text-sm font-medium">
                          {m.profile_import_local_file_size_label({
                            size: parseTraffic(form.watch('file')?.length).join(
                              '',
                            ),
                          })}
                        </div>
                      </FileDropZoneFileSelected>
                    </FileDropZone>
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
