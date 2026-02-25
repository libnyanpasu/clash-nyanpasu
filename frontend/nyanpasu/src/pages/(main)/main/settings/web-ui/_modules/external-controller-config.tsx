import { AnimatePresence } from 'framer-motion'
import { ChangeEvent, useCallback, useEffect } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
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
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

const formSchema = z.object({
  externalController: z.string(),
})

export default function ExternalControllerConfig() {
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
      await upsert.mutateAsync({
        'external-controller': data.externalController,
      })
      await refetch()

      // Wait for the server to apply
      await sleep(300)
      await runtimeProfile.refetch()
    },
    (error) => {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      })
    },
  )

  const handleReset = useCallback(() => {
    form.reset({
      externalController: data?.server || '',
    })
  }, [form, data?.server])

  return (
    <SettingsCard data-slot="external-controller-config-card">
      <SettingsCardContent
        className="px-2"
        data-slot="external-controller-config-card-content"
      >
        <form className="flex flex-col gap-2" onSubmit={handleSubmit}>
          <Controller
            control={form.control}
            name="externalController"
            render={({ field }) => {
              const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
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

                  {form.formState.errors.externalController && (
                    <SettingsCardAnimatedItem className="text-error">
                      {form.formState.errors.externalController.message}
                    </SettingsCardAnimatedItem>
                  )}
                </>
              )
            }}
          />

          <AnimatePresence initial={false}>
            {form.formState.isDirty && (
              <SettingsCardAnimatedItem>
                <div className="flex justify-end gap-2 pt-1">
                  <Button type="button" onClick={handleReset}>
                    {m.common_reset()}
                  </Button>

                  <Button
                    variant="raised"
                    onClick={handleSubmit}
                    loading={form.formState.isSubmitting}
                  >
                    {m.common_apply()}
                  </Button>
                </div>
              </SettingsCardAnimatedItem>
            )}
          </AnimatePresence>
        </form>
      </SettingsCardContent>
    </SettingsCard>
  )
}
