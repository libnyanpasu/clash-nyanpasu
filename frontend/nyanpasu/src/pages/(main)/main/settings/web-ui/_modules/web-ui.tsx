import AddIcon from '~icons/material-symbols/add-rounded'
import AllInboxRounded from '~icons/material-symbols/all-inbox-outline-rounded'
import DeleteRounded from '~icons/material-symbols/delete-rounded'
import EditSquareRounded from '~icons/material-symbols/edit-square-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import {
  ChangeEvent,
  PropsWithChildren,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { Controller, useForm } from 'react-hook-form'
import z from 'zod'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import TextMarquee from '@/components/ui/text-marquee'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { commands, useClashInfo, useSetting } from '@nyanpasu/interface'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

const useUrlLabels = () => {
  const { data } = useClashInfo()

  return useMemo(() => {
    let host = '127.0.0.1'
    let port = 7890

    if (data?.server) {
      const [h, p] = data.server.split(':')

      host = h
      port = Number(p)
    }

    return {
      host,
      port,
      secret: data?.secret,
    }
  }, [data])
}

const useFormattedUrl = (url: string) => {
  const labels = useUrlLabels()

  return useMemo(() => {
    let result = url

    for (const key of Object.keys(labels) as Array<keyof typeof labels>) {
      const regex = new RegExp(`%${key}`, 'g')

      result = result.replace(regex, String(labels[key] ?? ''))
    }

    return result
  }, [url, labels])
}

const PreviewItem = ({ url }: { url: string }) => {
  const formattedUrl = useFormattedUrl(url)

  return (
    <motion.div
      className="outline-outline-variant overflow-hidden rounded-2xl p-3 outline"
      initial={{
        height: 0,
        opacity: 0,
      }}
      animate={{
        height: 'auto',
        opacity: 1,
      }}
      exit={{
        height: 0,
        opacity: 0,
      }}
      transition={{
        height: {
          duration: 0.2,
          ease: 'easeInOut',
        },
        opacity: {
          duration: 0.15,
        },
      }}
    >
      <div>{m.settings_web_ui_preview_title()}</div>
      <TextMarquee className="w-full">{formattedUrl}</TextMarquee>
    </motion.div>
  )
}

const formSchema = z.object({
  url: z.httpUrl(),
})

const EditItemButton = ({
  defaultUrl,
  children,
}: PropsWithChildren<{ defaultUrl?: string }>) => {
  const [open, setOpen] = useState(false)

  const { value, upsert } = useSetting('web_ui_list')

  const labels = useUrlLabels()

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      url: defaultUrl,
    },
  })

  // sync default url to form
  useEffect(() => {
    form.reset({
      url: defaultUrl,
    })
  }, [defaultUrl, form])

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      form.reset({
        url: defaultUrl,
      })
    }

    setOpen(open)
  }

  const urlValue = form.watch('url')

  const handleSubmit = form.handleSubmit(
    async (data) => {
      try {
        await upsert([...(value || []), data.url])
        handleOpenChange(false)
      } catch (error) {
        message(formatError(error), {
          title: 'Error',
          kind: 'error',
        })
      }
    },
    (error) => {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      })
    },
  )

  return (
    <Modal open={open} onOpenChange={handleOpenChange}>
      <ModalTrigger asChild>{children}</ModalTrigger>

      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>{m.settings_web_ui_add_button()}</ModalTitle>
          </CardHeader>

          <CardContent>
            <Controller
              control={form.control}
              name="url"
              render={({ field }) => {
                const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
                  field.onChange(event.target.value)
                }

                return (
                  <>
                    <Input
                      variant="outlined"
                      label={m.settings_web_ui_input_label()}
                      value={field.value ?? ''}
                      onChange={handleChange}
                    />

                    {form.formState.errors.url && (
                      <SettingsCardAnimatedItem className="text-error">
                        {form.formState.errors.url.message}
                      </SettingsCardAnimatedItem>
                    )}
                  </>
                )
              }}
            />

            <p className="flex flex-wrap items-center gap-1 text-sm select-text">
              <span>{m.settings_web_ui_replace_with_label()}</span>

              {Object.entries(labels).map(([key], index) => {
                return (
                  <span
                    key={index}
                    className="bg-on-primary rounded-full px-2 py-0.5"
                  >
                    %{key}
                  </span>
                )
              })}
            </p>

            <AnimatePresence>
              {urlValue && <PreviewItem url={urlValue} />}
            </AnimatePresence>
          </CardContent>

          <CardFooter className="gap-2">
            <Button variant="flat" onClick={handleSubmit}>
              {m.common_submit()}
            </Button>

            <ModalClose>{m.common_cancel()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

const WebUIItem = ({ url }: { url: string }) => {
  const formattedUrl = useFormattedUrl(url)

  const handleOpen = useLockFn(async () => {
    await commands.openWebUrl(formattedUrl)
  })

  const { value, upsert } = useSetting('web_ui_list')

  const handleDelete = useLockFn(async () => {
    await upsert(value?.filter((item) => item !== url) || [])
  })

  return (
    <Card className="w-full min-w-0 space-y-4 overflow-hidden">
      <CardHeader className="flex w-full min-w-0 flex-row">
        <TextMarquee className="relative w-0 min-w-0 flex-1 text-base">
          {formattedUrl}
        </TextMarquee>
      </CardHeader>

      <CardFooter className="gap-1">
        <Button variant="flat" icon onClick={handleOpen}>
          <OpenInNewRounded className="size-5" />
        </Button>

        <EditItemButton defaultUrl={url}>
          <Button icon>
            <EditSquareRounded className="size-5" />
          </Button>
        </EditItemButton>

        <Button icon onClick={handleDelete}>
          <DeleteRounded className="size-5" />
        </Button>
      </CardFooter>
    </Card>
  )
}

const EmptyItem = () => {
  return (
    <Card variant="outline">
      <CardContent className="min-h-40 items-center justify-center">
        <AllInboxRounded className="size-10" />

        <p>{m.settings_web_ui_empty_item()}</p>
      </CardContent>
    </Card>
  )
}

export default function WebUI() {
  const { value } = useSetting('web_ui_list')

  return (
    <SettingsCard data-slot="web-ui-card">
      <SettingsCardContent
        data-slot="web-ui-card-content"
        className="flex min-w-0 flex-col gap-3 px-2"
      >
        <div className="px-1">{m.settings_web_ui_title()}</div>

        {value && value.length > 0 ? (
          value.map((item, index) => <WebUIItem key={index} url={item} />)
        ) : (
          <EmptyItem />
        )}

        <div className="flex justify-end">
          <EditItemButton>
            <Button
              className="flex items-center justify-center gap-1 px-4"
              variant="raised"
            >
              <AddIcon className="size-6" />
              <span>{m.settings_web_ui_add_button()}</span>
            </Button>
          </EditItemButton>
        </div>
      </SettingsCardContent>
    </SettingsCard>
  )
}
