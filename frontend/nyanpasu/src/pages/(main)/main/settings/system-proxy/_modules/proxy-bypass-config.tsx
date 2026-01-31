import { AnimatePresence } from 'framer-motion'
import { ChangeEvent, useCallback, useEffect } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
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

const DEFAULT_BYPASS =
  'localhost;127.;192.168.;10.;' +
  '172.16.;172.17.;172.18.;172.19.;172.20.;172.21.;172.22.;172.23.;' +
  '172.24.;172.25.;172.26.;172.27.;172.28.;172.29.;172.30.;172.31.*'

const formSchema = z.object({
  systemProxyBypass: z.string().nullable().optional(),
})

export default function ProxyBypassConfig() {
  const systemProxyBypass = useSetting('system_proxy_bypass')

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      systemProxyBypass: systemProxyBypass.value,
    },
  })

  useEffect(() => {
    form.setValue('systemProxyBypass', systemProxyBypass.value)
  }, [systemProxyBypass.value, form])

  const handleSubmit = form.handleSubmit(async (data) => {
    try {
      await systemProxyBypass.upsert(data.systemProxyBypass ?? DEFAULT_BYPASS)

      form.reset({
        systemProxyBypass: data.systemProxyBypass ?? DEFAULT_BYPASS,
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
      systemProxyBypass: systemProxyBypass.value,
    })
  }, [systemProxyBypass.value, form])

  return (
    <SettingsCard data-slot="proxy-guard-config-card">
      <SettingsCardContent
        className="px-2"
        data-slot="proxy-guard-config-card-content"
      >
        <form className="flex flex-col gap-2" onSubmit={handleSubmit}>
          <Controller
            control={form.control}
            name="systemProxyBypass"
            render={({ field }) => {
              const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
                field.onChange(event.target.value)
              }

              return (
                <>
                  <Input
                    variant="outlined"
                    label={m.settings_system_proxy_proxy_bypass_label()}
                    value={field.value ?? ''}
                    onChange={handleChange}
                  />

                  {form.formState.errors.systemProxyBypass && (
                    <SettingsCardAnimatedItem className="text-error">
                      {form.formState.errors.systemProxyBypass.message}
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
