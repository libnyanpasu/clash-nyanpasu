import { AnimatePresence } from 'framer-motion'
import { isNumber } from 'lodash-es'
import { useCallback, useEffect } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import { NumericInput } from '@/components/ui/input'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useSetting } from '@nyanpasu/interface'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

const formSchema = z.object({
  proxyGuardInterval: z.number().min(1),
})

export default function ProxyGuardConfig() {
  const proxyGuardInterval = useSetting('proxy_guard_interval')

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      proxyGuardInterval: proxyGuardInterval.value || 1,
    },
  })

  useEffect(() => {
    if (isNumber(proxyGuardInterval.value)) {
      form.setValue('proxyGuardInterval', proxyGuardInterval.value)
    }
  }, [proxyGuardInterval.value, form])

  const handleSubmit = form.handleSubmit(async (data) => {
    try {
      await proxyGuardInterval.upsert(data.proxyGuardInterval)

      form.reset({
        proxyGuardInterval: data.proxyGuardInterval,
      })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  const handleReset = useCallback(() => {
    form.reset({
      proxyGuardInterval: proxyGuardInterval.value || 1,
    })
  }, [proxyGuardInterval.value, form])

  return (
    <SettingsCard data-slot="proxy-guard-config-card">
      <SettingsCardContent
        className="px-2"
        data-slot="proxy-guard-config-card-content"
      >
        {/* <div className="border-surface-variant flex w-24 items-center justify-between rounded-md border p-2">
          <span>{proxyGuardInterval.value || 0}</span>
          <span>{m.unit_seconds()}</span>
        </div> */}
        <form className="flex flex-col gap-2" onSubmit={handleSubmit}>
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
                    value={field.value || 0}
                    onChange={handleChange}
                  />

                  <AnimatePresence initial={false}>
                    {form.formState.errors.proxyGuardInterval && (
                      <SettingsCardAnimatedItem className="text-error">
                        {form.formState.errors.proxyGuardInterval.message}
                      </SettingsCardAnimatedItem>
                    )}
                  </AnimatePresence>
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
