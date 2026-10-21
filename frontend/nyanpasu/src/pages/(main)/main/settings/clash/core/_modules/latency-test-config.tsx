import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { AnimatePresence } from 'motion/react'
import { useEffect, useId, useState, type BaseSyntheticEvent } from 'react'
import { Controller, useForm, type UseFormReturn } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input, NumericInput } from '@nyanpasu/ui/input'
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
import { zodResolver } from '@hookform/resolvers/zod'
import { DEFAULT_LATENCY_TEST_URL } from '@nyanpasu/constants'
import { useSetting } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../../_modules/settings-card'

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

const LatencyUrlForm = ({
  form,
  onSubmit,
}: {
  form: UseFormReturn<UrlFormValues>
  onSubmit: (event?: BaseSyntheticEvent) => Promise<unknown>
}) => {
  const id = useId()
  const errorId = `${id}-url-error`

  return (
    <form
      className="flex flex-col gap-2"
      data-slot="latency-test-url-form"
      onSubmit={onSubmit}
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
              aria-describedby={form.formState.errors.url ? errorId : undefined}
            />

            <ItemLabelDescription className="px-4">
              {m.settings_clash_latency_test_url_description()}
            </ItemLabelDescription>

            <AnimatePresence initial={false}>
              {form.formState.errors.url && (
                <SettingsCardAnimatedItem id={errorId} className="text-error">
                  {form.formState.errors.url.message}
                </SettingsCardAnimatedItem>
              )}
            </AnimatePresence>
          </>
        )}
      />
    </form>
  )
}

const LatencyTimeoutForm = ({
  form,
  onSubmit,
}: {
  form: UseFormReturn<TimeoutFormValues>
  onSubmit: (event?: BaseSyntheticEvent) => Promise<unknown>
}) => {
  const id = useId()
  const errorId = `${id}-timeout-error`

  return (
    <form
      className="flex flex-col gap-2"
      data-slot="latency-test-timeout-form"
      onSubmit={onSubmit}
    >
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
                form.formState.errors.timeout ? errorId : undefined
              }
            />

            <AnimatePresence initial={false}>
              {form.formState.errors.timeout && (
                <SettingsCardAnimatedItem id={errorId} className="text-error">
                  {form.formState.errors.timeout.message}
                </SettingsCardAnimatedItem>
              )}
            </AnimatePresence>
          </>
        )}
      />
    </form>
  )
}

export default function LatencyTestConfig() {
  const [open, setOpen] = useState(false)

  const latencyUrl = useSetting('default_latency_test')
  const latencyTimeoutMs = useSetting('default_latency_timeout_ms')

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

  const submitUrl = urlForm.handleSubmit(async (data) => {
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
  })

  const submitTimeout = timeoutForm.handleSubmit(async (data) => {
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
  })

  const handleOpenChange = (next: boolean) => {
    if (next) {
      urlForm.reset({ url: savedUrl })
      timeoutForm.reset({ timeout: savedSeconds })
    }

    setOpen(next)
  }

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
    <SettingsCard data-slot="latency-test-config">
      <Modal open={open} onOpenChange={handleOpenChange}>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_latency_test_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {`${savedUrl || DEFAULT_LATENCY_TEST_URL} · ${savedSeconds}s`}
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
              <ModalTitle>{m.settings_clash_latency_test_label()}</ModalTitle>
            </CardHeader>

            <CardContent className="flex flex-col gap-6">
              <LatencyUrlForm form={urlForm} onSubmit={submitUrl} />

              <LatencyTimeoutForm form={timeoutForm} onSubmit={submitTimeout} />
            </CardContent>

            <CardFooter className="gap-2">
              <Button
                variant="flat"
                onClick={handleApply}
                loading={isSubmitting}
                disabled={!isDirty}
              >
                {m.common_apply()}
              </Button>

              <Button onClick={handleReset} disabled={!isDirty}>
                {m.common_reset()}
              </Button>

              <ModalClose>{m.common_close()}</ModalClose>
            </CardFooter>
          </Card>
        </ModalContent>
      </Modal>
    </SettingsCard>
  )
}
