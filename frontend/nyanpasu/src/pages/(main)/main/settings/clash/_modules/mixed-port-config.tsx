import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { AnimatePresence } from 'framer-motion'
import { useEffect, useMemo, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { NumericInput } from '@/components/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useClashConfig, useSetting } from '@nyanpasu/interface'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

const DEFAULT_MIXED_PORT = 7890

const formSchema = z.object({
  mixedPort: z.number().min(1).max(65535),
})

export default function MixedPortConfig() {
  const [open, setOpen] = useState(false)

  const mixedPort = useSetting('verge_mixed_port')

  const clashConfig = useClashConfig()

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      mixedPort: DEFAULT_MIXED_PORT,
    },
  })

  // get current mixed port from clash config or verge setting
  const currentMixedPort = useMemo(() => {
    return (
      clashConfig.query.data?.['mixed-port'] ||
      mixedPort.value ||
      DEFAULT_MIXED_PORT
    )
  }, [clashConfig.query.data, mixedPort.value])

  // sync current mixed port to form
  useEffect(() => {
    form.setValue('mixedPort', currentMixedPort)
  }, [currentMixedPort, form])

  const handleSubmit = form.handleSubmit(async (data) => {
    try {
      await clashConfig.upsert.mutateAsync({
        'mixed-port': data.mixedPort,
      })
      await mixedPort.upsert(data.mixedPort)

      form.reset({
        mixedPort: data.mixedPort,
      })

      setOpen(false)
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <SettingsCard data-slot="mixed-port-config-card">
      <Modal open={open} onOpenChange={setOpen}>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_mixed_port_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {m.settings_clash_settings_mixed_port_label_value({
                      port: currentMixedPort,
                    })}
                  </ItemLabelDescription>
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
                {m.settings_clash_settings_mixed_port_label_edit()}
              </ModalTitle>
            </CardHeader>

            <CardContent asChild>
              <form className="flex flex-col gap-2" onSubmit={handleSubmit}>
                <Controller
                  name="mixedPort"
                  control={form.control}
                  render={({ field, fieldState }) => {
                    const handleChange = (value: number | null) => {
                      field.onChange(value)
                    }

                    return (
                      <>
                        <NumericInput
                          variant="outlined"
                          label={m.settings_clash_settings_mixed_port_label()}
                          value={field.value}
                          onChange={handleChange}
                          allowNegative={false}
                          decimalScale={0}
                        />

                        <AnimatePresence>
                          {fieldState.error && (
                            <SettingsCardAnimatedItem className="text-error">
                              {fieldState.error.message}
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
