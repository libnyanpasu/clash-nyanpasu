import { AnimatePresence } from 'motion/react'
import { useCallback, useEffect, useId } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { Input, NumericInput } from '@nyanpasu/ui/input'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { DEFAULT_LATENCY_TEST_URL } from '@nyanpasu/constants'
import { useSetting } from '@nyanpasu/query'
import {
  ItemLabelDescription,
  SettingsCardAnimatedItem,
} from '../../_modules/settings-card'

const MS_PER_SECOND = 1000

const MAX_TIMEOUT_SECONDS = 30

// `URL.canParse` is missing from WebKit before 17 (macOS 12/13 webviews).
const isHttpUrl = (value: string) => {
  try {
    return ['http:', 'https:'].includes(new URL(value).protocol)
  } catch {
    return false
  }
}

const formSchema = z.object({
  // An empty URL restores the default on save.
  url: z
    .string()
    .trim()
    .refine((value) => value === '' || isHttpUrl(value), {
      message: m.settings_clash_latency_test_invalid_url(),
    }),
  timeout: z
    .number(m.settings_clash_latency_test_invalid_timeout())
    .min(1, m.settings_clash_latency_test_invalid_timeout())
    .max(MAX_TIMEOUT_SECONDS, m.settings_clash_latency_test_invalid_timeout()),
})

type FormValues = z.infer<typeof formSchema>

export default function LatencyTestConfig() {
  const id = useId()
  const urlErrorId = `${id}-url-error`
  const timeoutErrorId = `${id}-timeout-error`

  const latencyUrl = useSetting('default_latency_test')
  const latencyTimeoutMs = useSetting('default_latency_timeout_ms')

  const savedValues: FormValues = {
    url: latencyUrl.value ?? '',
    timeout: (latencyTimeoutMs.value ?? 0) / MS_PER_SECOND,
  }

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: savedValues,
  })

  useEffect(() => {
    if (!form.formState.isDirty) {
      form.reset({
        url: latencyUrl.value ?? '',
        timeout: (latencyTimeoutMs.value ?? 0) / MS_PER_SECOND,
      })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [latencyUrl.value, latencyTimeoutMs.value])

  const handleSubmit = form.handleSubmit(async (data) => {
    const url = data.url || DEFAULT_LATENCY_TEST_URL

    try {
      if (url !== latencyUrl.value) {
        await latencyUrl.upsert(url)
      }

      if (data.timeout * MS_PER_SECOND !== latencyTimeoutMs.value) {
        await latencyTimeoutMs.upsert(Math.round(data.timeout * MS_PER_SECOND))
      }

      form.reset({ ...data, url })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  const handleReset = useCallback(() => {
    form.reset({
      url: latencyUrl.value ?? '',
      timeout: (latencyTimeoutMs.value ?? 0) / MS_PER_SECOND,
    })
  }, [latencyUrl.value, latencyTimeoutMs.value, form])

  return (
    <form
      className="flex flex-col gap-2"
      data-slot="latency-test-config"
      onSubmit={handleSubmit}
    >
      <Controller
        name="url"
        control={form.control}
        render={({ field }) => (
          <>
            <Input
              variant="outlined"
              label={m.settings_clash_latency_test_url_label()}
              aria-label={m.settings_clash_latency_test_url_label()}
              value={field.value}
              onChange={(event) => field.onChange(event.target.value)}
              aria-invalid={!!form.formState.errors.url}
              aria-describedby={
                form.formState.errors.url ? urlErrorId : undefined
              }
            />

            <ItemLabelDescription className="px-4">
              {m.settings_clash_latency_test_url_description()}
            </ItemLabelDescription>

            <AnimatePresence initial={false}>
              {form.formState.errors.url && (
                <SettingsCardAnimatedItem
                  id={urlErrorId}
                  className="text-error"
                >
                  {form.formState.errors.url.message}
                </SettingsCardAnimatedItem>
              )}
            </AnimatePresence>
          </>
        )}
      />

      <Controller
        name="timeout"
        control={form.control}
        render={({ field }) => (
          <>
            <NumericInput
              variant="outlined"
              label={m.settings_clash_latency_test_timeout_label()}
              aria-label={m.settings_clash_latency_test_timeout_label()}
              value={field.value}
              onChange={(value) => field.onChange(value)}
              allowNegative={false}
              aria-invalid={!!form.formState.errors.timeout}
              aria-describedby={
                form.formState.errors.timeout ? timeoutErrorId : undefined
              }
            />

            <AnimatePresence initial={false}>
              {form.formState.errors.timeout && (
                <SettingsCardAnimatedItem
                  id={timeoutErrorId}
                  className="text-error"
                >
                  {form.formState.errors.timeout.message}
                </SettingsCardAnimatedItem>
              )}
            </AnimatePresence>
          </>
        )}
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
  )
}
