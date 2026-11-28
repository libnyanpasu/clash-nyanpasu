import { useEffect, useId } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import {
  Dialog,
  DialogTrigger,
  FormDialogContent,
  FormDialogError,
  useDialog,
} from '@nyanpasu/ui/dialog'
import { Input, NumericInput } from '@nyanpasu/ui/input'
import { ItemAction } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { DEFAULT_LATENCY_TEST_URL } from '@nyanpasu/constants'
import { useLockFn } from '@nyanpasu/hooks'
import { useFullSetting } from '@nyanpasu/query'

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

const urlFormSchema = z.object({
  // An empty URL restores the default on save.
  url: z
    .string()
    .trim()
    .refine((value) => value === '' || isHttpUrl(value), {
      message: m.settings_clash_latency_test_invalid_url(),
    }),
})

const timeoutFormSchema = z.object({
  timeout: z
    .number(m.settings_clash_latency_test_invalid_timeout())
    .min(1, m.settings_clash_latency_test_invalid_timeout())
    .max(MAX_TIMEOUT_SECONDS, m.settings_clash_latency_test_invalid_timeout()),
})

type UrlFormValues = z.infer<typeof urlFormSchema>

type TimeoutFormValues = z.infer<typeof timeoutFormSchema>

const reportError = (error: unknown) =>
  message(formatError(error), { title: 'Error', kind: 'error', error })

export default function LatencyTestConfig() {
  return (
    <Dialog>
      <LatencyTestConfigForm />
    </Dialog>
  )
}

function LatencyTestConfigForm() {
  const { setOpen } = useDialog()
  const id = useId()
  const urlErrorId = `${id}-url-error`
  const timeoutErrorId = `${id}-timeout-error`

  const latencyUrl = useFullSetting('default_latency_test')
  const latencyTimeoutMs = useFullSetting('default_latency_timeout_ms')

  const savedUrl = latencyUrl.value ?? ''
  const savedTimeoutMs = latencyTimeoutMs.value ?? 0
  const savedSeconds = savedTimeoutMs / MS_PER_SECOND

  const urlForm = useForm<UrlFormValues>({
    resolver: zodResolver(urlFormSchema),
    defaultValues: { url: savedUrl },
  })

  const timeoutForm = useForm<TimeoutFormValues>({
    resolver: zodResolver(timeoutFormSchema),
    defaultValues: { timeout: savedSeconds },
  })

  const isDirty = urlForm.formState.isDirty || timeoutForm.formState.isDirty
  const isSubmitting =
    urlForm.formState.isSubmitting || timeoutForm.formState.isSubmitting

  useEffect(() => {
    if (!urlForm.formState.isDirty) {
      urlForm.reset({ url: savedUrl })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [savedUrl])

  useEffect(() => {
    if (!timeoutForm.formState.isDirty) {
      timeoutForm.reset({ timeout: savedSeconds })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [savedTimeoutMs])

  const submitUrl = useLockFn(
    urlForm.handleSubmit(async (data) => {
      const url = data.url || DEFAULT_LATENCY_TEST_URL

      try {
        if (url !== savedUrl) {
          await latencyUrl.upsert(url)
        }

        urlForm.reset({ url })

        return true
      } catch (error) {
        reportError(error)

        return false
      }
    }),
  )

  const submitTimeout = useLockFn(
    timeoutForm.handleSubmit(async (data) => {
      const ms = Math.round(data.timeout * MS_PER_SECOND)

      try {
        if (ms !== savedTimeoutMs) {
          await latencyTimeoutMs.upsert(ms)
        }

        timeoutForm.reset({ timeout: data.timeout })

        return true
      } catch (error) {
        reportError(error)

        return false
      }
    }),
  )

  const handleReset = () => {
    urlForm.reset({ url: savedUrl })
    timeoutForm.reset({ timeout: savedSeconds })
  }

  // A submit resolves `undefined` when validation rejects its form, so the
  // dialog closes only after both forms are saved.
  const handleApply = async () => {
    const saved = await Promise.all([submitUrl(), submitTimeout()])

    if (saved.every(Boolean)) {
      setOpen(false)
    }
  }

  return (
    <>
      <DialogTrigger asChild>
        <ItemAction
          data-slot="latency-test-config"
          title={m.settings_clash_latency_test_label()}
          description={`${savedUrl || DEFAULT_LATENCY_TEST_URL} · ${savedSeconds}s`}
        />
      </DialogTrigger>

      <FormDialogContent
        title={m.settings_clash_latency_test_label()}
        submitLabel={m.common_apply()}
        cancelLabel={m.common_close()}
        submitting={isSubmitting}
        submitDisabled={!isDirty}
        extraActions={
          <Button
            type="button"
            variant="basic"
            onClick={handleReset}
            disabled={!isDirty || isSubmitting}
          >
            {m.common_reset()}
          </Button>
        }
        onOpen={handleReset}
        onSubmit={handleApply}
      >
        <div className="flex flex-col gap-2" data-slot="latency-test-url-form">
          <Controller
            name="url"
            control={urlForm.control}
            render={({ field }) => {
              const error = urlForm.formState.errors.url

              return (
                <>
                  <Input
                    label={m.settings_clash_latency_test_url_label()}
                    aria-label={m.settings_clash_latency_test_url_label()}
                    value={field.value}
                    onChange={(event) => field.onChange(event.target.value)}
                    aria-invalid={Boolean(error)}
                    aria-describedby={error ? urlErrorId : undefined}
                  />

                  <p className="text-on-surface-variant text-sm">
                    {m.settings_clash_latency_test_url_description()}
                  </p>

                  <FormDialogError id={urlErrorId} message={error?.message} />
                </>
              )
            }}
          />
        </div>

        <div
          className="flex flex-col gap-2"
          data-slot="latency-test-timeout-form"
        >
          <Controller
            name="timeout"
            control={timeoutForm.control}
            render={({ field }) => {
              const error = timeoutForm.formState.errors.timeout

              return (
                <>
                  <NumericInput
                    label={m.settings_clash_latency_test_timeout_label()}
                    aria-label={m.settings_clash_latency_test_timeout_label()}
                    value={field.value}
                    onChange={(value) => field.onChange(value)}
                    allowNegative={false}
                    aria-invalid={Boolean(error)}
                    aria-describedby={error ? timeoutErrorId : undefined}
                  />

                  <FormDialogError
                    id={timeoutErrorId}
                    message={error?.message}
                  />
                </>
              )
            }}
          />
        </div>
      </FormDialogContent>
    </>
  )
}
