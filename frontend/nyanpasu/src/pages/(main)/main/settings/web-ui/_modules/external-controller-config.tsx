import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { AnimatePresence } from 'framer-motion'
import { ChangeEvent, useEffect, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
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
import { m } from '@/paraglide/messages'
import { formatError, sleep } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import {
  useClashConfig,
  useClashInfo,
  useRuntimeProfile,
} from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

const formSchema = z.object({
  externalController: z.string(),
})

export default function ExternalControllerConfig() {
  const [open, setOpen] = useState(false)

  const { data, refetch } = useClashInfo()

  const { upsert } = useClashConfig()

  const runtimeProfile = useRuntimeProfile()

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      externalController: data?.server || '',
    },
  })

  useEffect(() => {
    form.reset({
      externalController: data?.server || '',
    })
  }, [data?.server, form])

  const handleSubmit = form.handleSubmit(
    async (data) => {
      try {
        await upsert.mutateAsync({
          'external-controller': data.externalController,
        })
        await refetch()

        // Wait for the server to apply
        await sleep(300)
        await runtimeProfile.refetch()

        setOpen(false)
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
    <SettingsCard data-slot="external-controller-config-card">
      <Modal open={open} onOpenChange={setOpen}>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_external_controll_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>{data?.server}</ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </ModalTrigger>
        </SettingsCardContent>

        <ModalContent>
          <Card className="flex min-w-96 flex-col">
            <CardHeader>
              <ModalTitle>
                {m.settings_clash_settings_external_controll_label_edit()}
              </ModalTitle>
            </CardHeader>

            <CardContent asChild>
              <form className="flex flex-col gap-2" onSubmit={handleSubmit}>
                <Controller
                  control={form.control}
                  name="externalController"
                  render={({ field }) => {
                    const handleChange = (
                      event: ChangeEvent<HTMLInputElement>,
                    ) => {
                      field.onChange(event.target.value)
                    }

                    return (
                      <>
                        <Input
                          variant="outlined"
                          label={m.settings_clash_settings_external_controll_label()}
                          value={field.value ?? ''}
                          onChange={handleChange}
                        />

                        <AnimatePresence>
                          {form.formState.errors.externalController && (
                            <SettingsCardAnimatedItem className="text-error">
                              {form.formState.errors.externalController.message}
                            </SettingsCardAnimatedItem>
                          )}
                        </AnimatePresence>
                      </>
                    )
                  }}
                />
              </form>
            </CardContent>

            <CardFooter className="gap-2">
              <Button
                variant="flat"
                onClick={handleSubmit}
                loading={form.formState.isSubmitting}
              >
                {m.common_apply()}
              </Button>

              <ModalClose>{m.common_close()}</ModalClose>
            </CardFooter>
          </Card>
        </ModalContent>
      </Modal>
    </SettingsCard>
  )
}
