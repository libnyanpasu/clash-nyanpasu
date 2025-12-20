import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useMemo } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import { NumericInput } from '@/components/ui/input'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useClashConfig, useSetting } from '@nyanpasu/interface'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const DEFAULT_MIXED_PORT = 7890

const formSchema = z.object({
  mixedPort: z.number().min(1).max(65535),
})

export default function MixedPortConfig() {
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
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  const handleReset = useCallback(() => {
    form.reset({
      mixedPort: currentMixedPort,
    })
  }, [form, currentMixedPort])

  return (
    <SettingsCard data-slot="mixed-port-config-card">
      <SettingsCardContent
        className="px-2"
        data-slot="mixed-port-config-card-content"
      >
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
                      <motion.p
                        className="overflow-hidden text-red-500"
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
                        {fieldState.error.message}
                      </motion.p>
                    )}
                  </AnimatePresence>
                </>
              )
            }}
          />

          <AnimatePresence initial={false}>
            {form.formState.isDirty && (
              <motion.div
                className="overflow-hidden"
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
              </motion.div>
            )}
          </AnimatePresence>
        </form>
      </SettingsCardContent>
    </SettingsCard>
  )
}
