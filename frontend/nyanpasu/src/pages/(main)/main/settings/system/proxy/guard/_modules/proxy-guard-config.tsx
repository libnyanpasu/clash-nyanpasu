import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { AnimatePresence } from 'motion/react'
import { useId } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { NumericInput } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import { useSystemSetting } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCardAnimatedItem,
} from '../../../../../_modules/settings-card'

const formSchema = z.object({
  proxyGuardInterval: z.number().min(1),
})

export default function ProxyGuardConfig() {
  return (
    <Modal>
      <ProxyGuardConfigForm />
    </Modal>
  )
}

function ProxyGuardConfigForm() {
  const { setOpen } = useModal()
  const errorId = useId()
  const proxyGuardInterval = useSystemSetting('proxy_guard_interval')

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      proxyGuardInterval: proxyGuardInterval.value || 1,
    },
  })

  const handleSubmit = useLockFn(
    form.handleSubmit(async (data) => {
      try {
        await proxyGuardInterval.upsert(data.proxyGuardInterval)

        setOpen(false)
      } catch (error) {
        message(formatError(error), {
          title: 'Error',
          kind: 'error',
          error,
        })
      }
    }),
  )

  const handleReset = () => {
    form.reset({
      proxyGuardInterval: proxyGuardInterval.value || 1,
    })
  }

  const { isDirty, isSubmitting } = form.formState

  return (
    <>
      <ModalTrigger asChild onClick={handleReset}>
        <Button className="text-on-surface! h-auto w-full text-left text-base">
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>
                {m.settings_system_proxy_proxy_guard_interval_label()}
              </ItemLabelText>
              <ItemLabelDescription>
                {proxyGuardInterval.value || 1}
              </ItemLabelDescription>
            </ItemLabel>
            <ArrowForwardIosRounded />
          </ItemContainer>
        </Button>
      </ModalTrigger>
      <ModalContent dismissible={!isSubmitting}>
        <Card className="flex min-w-96 flex-col">
          <CardHeader>
            <ModalTitle>
              {m.settings_system_proxy_proxy_guard_interval_label()}
            </ModalTitle>
          </CardHeader>
          <form
            data-slot="proxy-guard-form"
            onSubmit={(event) => {
              event.preventDefault()
              handleSubmit(event)
            }}
          >
            <CardContent className="flex flex-col gap-2">
              <Controller
                name="proxyGuardInterval"
                control={form.control}
                render={({ field }) => {
                  const handleChange = (value: number | null) => {
                    field.onChange(value)
                  }

                  return (
                    <>
                      <NumericInput
                        variant="outlined"
                        label={m.settings_system_proxy_proxy_guard_interval_label()}
                        value={field.value}
                        disabled={isSubmitting}
                        aria-label={m.settings_system_proxy_proxy_guard_interval_label()}
                        aria-invalid={
                          !!form.formState.errors.proxyGuardInterval
                        }
                        aria-describedby={
                          form.formState.errors.proxyGuardInterval
                            ? errorId
                            : undefined
                        }
                        onChange={handleChange}
                      />

                      <AnimatePresence initial={false}>
                        {form.formState.errors.proxyGuardInterval && (
                          <SettingsCardAnimatedItem
                            id={errorId}
                            className="text-error"
                          >
                            {form.formState.errors.proxyGuardInterval.message}
                          </SettingsCardAnimatedItem>
                        )}
                      </AnimatePresence>
                    </>
                  )
                }}
              />
            </CardContent>
            <CardFooter className="gap-2">
              <Button
                type="submit"
                variant="flat"
                loading={isSubmitting}
                disabled={!isDirty || isSubmitting}
              >
                {m.common_apply()}
              </Button>
              <Button
                type="button"
                onClick={handleReset}
                disabled={!isDirty || isSubmitting}
              >
                {m.common_reset()}
              </Button>
              <ModalClose type="button" disabled={isSubmitting}>
                {m.common_close()}
              </ModalClose>
            </CardFooter>
          </form>
        </Card>
      </ModalContent>
    </>
  )
}
