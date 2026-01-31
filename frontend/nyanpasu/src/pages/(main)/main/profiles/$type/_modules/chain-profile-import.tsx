import NoteStackAddRounded from '~icons/material-symbols/note-stack-add-rounded'
import dayjs from 'dayjs'
import { AnimatePresence } from 'framer-motion'
import { useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import z from 'zod'
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
import { ScrollArea } from '@/components/ui/scroll-area'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import {
  MergeProfileBuilder,
  ProfileTemplate,
  ScriptProfileBuilder,
  useProfile,
} from '@nyanpasu/interface'
import {
  PROFILE_TYPE_NAMES,
  PROFILE_TYPES,
  ProfileType as RawProfileType,
} from '../../_modules/consts'
import AnimatedErrorItem from '../../_modules/error-item'
import { Route as IndexRoute } from '../index'

const formSchema = z.object({
  type: z.enum(['merge', 'script']),
  uid: z.string().nullable(),
  name: z.string().nullable(),
  file: z.string().nullable(),
  desc: z.string().nullable(),
  updated: z.number().nullable(),
  script_type: z.literal('javascript').or(z.literal('lua')).nullable(),
}) satisfies z.ZodType<MergeProfileBuilder | ScriptProfileBuilder>

const getDefaultValues = (rawType: RawProfileType) => {
  // get the first type of the raw type
  // FIXME: better error handling
  const finallyType = PROFILE_TYPES[rawType][0]

  // check if the type is script
  const typeValidation = formSchema.shape.type.safeParse(finallyType.type)
  if (!typeValidation.success) {
    throw new Error(typeValidation.error.message)
  }

  // check if script_type is valid
  const scriptTypeValue =
    'script_type' in finallyType ? finallyType.script_type : null
  const scriptTypeValidation =
    formSchema.shape.script_type.safeParse(scriptTypeValue)
  if (!scriptTypeValidation.success) {
    throw new Error(scriptTypeValidation.error.message)
  }

  return {
    type: typeValidation.data,
    uid: null,
    name: `${PROFILE_TYPE_NAMES[rawType]} - ${dayjs().format('YYYY-MM-DD HH:mm:ss')}`,
    file: null,
    desc: null,
    updated: null,
    script_type: scriptTypeValidation.data,
  } satisfies z.infer<typeof formSchema>
}

export default function ChainProfileImport() {
  const [open, setOpen] = useState(false)

  const { type } = IndexRoute.useParams()

  const { create } = useProfile()

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: getDefaultValues(type as RawProfileType),
  })

  const blockTask = useBlockTask(
    `create-chain-profile`,
    form.handleSubmit(async (data) => {
      try {
        await create.mutateAsync({
          type: 'manual',
          data: {
            item: data,
            // TODO: when content editor is implemented, use the content editor value instead of the template
            fileData: ProfileTemplate[type as keyof typeof ProfileTemplate],
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
      form.reset(getDefaultValues(type as RawProfileType))
    }
  }

  const handleSubmit = useLockFn(blockTask.execute)

  return (
    <Modal open={open} onOpenChange={handleToggle}>
      <ModalTrigger asChild>
        <Button variant="fab" icon>
          <NoteStackAddRounded className="size-6" />
        </Button>
      </ModalTrigger>

      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>
              {m.profile_import_chain_title({
                type: PROFILE_TYPE_NAMES[type as RawProfileType],
              })}
            </ModalTitle>
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

                {/* TODO: edit content before submit */}
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
