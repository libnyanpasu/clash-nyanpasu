import AddIcon from '~icons/material-symbols/add-rounded'
import DeleteRounded from '~icons/material-symbols/delete-rounded'
import EditSquareRounded from '~icons/material-symbols/edit-square-rounded'
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { ChangeEvent, PropsWithChildren, useMemo } from 'react'
import { Controller, useForm } from 'react-hook-form'
import z from 'zod'
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
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { commands } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import { useClashInfo, useClashSetting } from '@nyanpasu/query'
import { SettingsCardAnimatedItem } from '../../../../_modules/settings-card'
import {
  SettingsList,
  SettingsListActions,
  SettingsListContent,
  SettingsListItem,
} from '../../../../_modules/settings-list'

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

const EditItemButton = (props: PropsWithChildren<{ defaultUrl?: string }>) => (
  <Modal>
    <EditItemForm {...props} />
  </Modal>
)

const EditItemForm = ({
  defaultUrl,
  children,
}: PropsWithChildren<{ defaultUrl?: string }>) => {
  const { setOpen } = useModal()

  const { value, upsert } = useClashSetting('web_ui_list')

  const labels = useUrlLabels()

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      url: defaultUrl,
    },
  })

  const urlValue = form.watch('url')

  const handleSubmit = useLockFn(
    form.handleSubmit(
      async (data) => {
        try {
          await upsert([...(value || []), data.url])
          setOpen(false)
        } catch (error) {
          message(formatError(error), {
            title: 'Error',
            kind: 'error',
            error,
          })
        }
      },
      (error) => {
        message(formatError(error), {
          title: 'Error',
          kind: 'error',
          error,
        })
      },
    ),
  )

  return (
    <>
      <ModalTrigger asChild>{children}</ModalTrigger>

      <ModalContent
        dismissible={!form.formState.isSubmitting}
        onOpenAutoFocus={() => form.reset({ url: defaultUrl })}
      >
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
            <Button
              variant="flat"
              onClick={handleSubmit}
              loading={form.formState.isSubmitting}
              disabled={form.formState.isSubmitting}
            >
              {m.common_submit()}
            </Button>

            <ModalClose disabled={form.formState.isSubmitting}>
              {m.common_cancel()}
            </ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </>
  )
}

const WebUIItem = ({ url }: { url: string }) => {
  const formattedUrl = useFormattedUrl(url)

  const handleOpen = useLockFn(async () => {
    await commands.openWebUrl(formattedUrl)
  })

  const { value, upsert } = useClashSetting('web_ui_list')

  const handleDelete = useLockFn(async () => {
    await upsert(value?.filter((item) => item !== url) || [])
  })

  return (
    <SettingsListItem data-slot="web-ui-item">
      <TextMarquee className="relative w-0 min-w-0 flex-1 text-base">
        {formattedUrl}
      </TextMarquee>

      <div
        className="flex shrink-0 items-center gap-1"
        data-slot="web-ui-item-actions"
      >
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
      </div>
    </SettingsListItem>
  )
}

export default function WebUI() {
  const { value } = useClashSetting('web_ui_list')

  return (
    <SettingsList data-slot="web-ui-card">
      <SettingsListContent
        data-slot="web-ui-card-content"
        empty={!value?.length}
        emptyMessage={m.settings_web_ui_empty_item()}
      >
        {value?.map((item, index) => (
          <WebUIItem key={index} url={item} />
        ))}
      </SettingsListContent>

      <SettingsListActions>
        <EditItemButton>
          <Button
            className="flex items-center justify-center gap-1 px-4"
            variant="raised"
          >
            <AddIcon className="size-6" />
            <span>{m.settings_web_ui_add_button()}</span>
          </Button>
        </EditItemButton>
      </SettingsListActions>
    </SettingsList>
  )
}
