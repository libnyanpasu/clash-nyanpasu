import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { AnimatePresence } from 'motion/react'
import { ChangeEvent, useState } from 'react'
import { Controller } from 'react-hook-form'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  formatControllerAddress,
  useClashInfo,
  useClashSetting,
  useRuntimeProfile,
} from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'
import { useExternalControllerForm } from './use-external-controller-form'

export default function ExternalControllerConfig() {
  const [open, setOpen] = useState(false)

  const { data, refetch } = useClashInfo()

  const externalController = useClashSetting('external_controller')

  // Edit the configured address, not the reported one: the reported server
  // turns an unspecified bind host into loopback and may carry a port the core
  // picked at runtime.
  const configuredAddress = externalController.value
    ? formatControllerAddress(externalController.value)
    : ''

  const runtimeProfile = useRuntimeProfile()

  const { form, handleSubmit } = useExternalControllerForm(
    configuredAddress,
    async (address) => {
      try {
        await externalController.upsert({
          host: address.host,
          port: { start_port: address.port },
        })
        await refetch()

        await runtimeProfile.refetch()

        setOpen(false)
      } catch (error) {
        message(formatError(error), {
          title: 'Error',
          kind: 'error',
          error,
        })
      }
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
