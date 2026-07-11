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
  ProfileTemplate,
  useProfile,
  type NewProfileRequest_Deserialize,
  type ProfileDefinition_Deserialize,
} from '@nyanpasu/interface'
import {
  PROFILE_TYPE_NAMES,
  ProfileType as RawProfileType,
} from '../../_modules/consts'
import AnimatedErrorItem from '../../_modules/error-item'
import { Route as IndexRoute } from '../index'

const formSchema = z.object({
  name: z.string().nullable(),
  desc: z.string().nullable(),
})

const getDefaultValues = (rawType: RawProfileType) => {
  return {
    name: `${PROFILE_TYPE_NAMES[rawType]} - ${dayjs().format('YYYY-MM-DD HH:mm:ss')}`,
    desc: null,
  } satisfies z.infer<typeof formSchema>
}

/** Build a Transform request (Overlay/Script) + its seed template for the tab. */
const buildTransformRequest = (
  rawType: RawProfileType,
  name: string,
  desc: string | null,
): { request: NewProfileRequest_Deserialize; fileData: string } => {
  const source = {
    type: 'local' as const,
    binding: {
      type: 'managed' as const,
      file: 'pending.yaml',
    },
  }

  let definition: ProfileDefinition_Deserialize
  let fileData: string
  if (rawType === RawProfileType.Merge) {
    definition = {
      type: 'transform',
      transform: { type: 'overlay', source },
    }
    fileData = ProfileTemplate.merge
  } else if (rawType === RawProfileType.Lua) {
    definition = {
      type: 'transform',
      transform: { type: 'script', source, runtime: 'lua' },
    }
    fileData = ProfileTemplate.luascript
  } else {
    definition = {
      type: 'transform',
      transform: { type: 'script', source, runtime: 'javascript' },
    }
    fileData = ProfileTemplate.javascript
  }

  return {
    request: { metadata: { name, desc }, definition },
    fileData,
  }
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
        // TODO: when content editor is implemented, use the content editor value instead of the template
        const { request, fileData } = buildTransformRequest(
          type as RawProfileType,
          data.name ?? '',
          data.desc ?? null,
        )
        await create.mutateAsync({
          type: 'manual',
          data: { request, fileData },
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
            <ScrollArea className="max-h-[80dvh]">
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
