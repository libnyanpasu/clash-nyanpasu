import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import { useId } from 'react'
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
import { Input } from '@nyanpasu/ui/input'
import { ItemAction } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { writeClipboardText } from '@/utils/clipboard'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import {
  useClashAccess,
  useClashInfo,
  useRuntimeProfile,
} from '@nyanpasu/query'

const formSchema = z.object({
  coreSecret: z.string(),
})

export default function CoreSecretConfig() {
  return (
    <Dialog>
      <CoreSecretConfigForm />
    </Dialog>
  )
}

function CoreSecretConfigForm() {
  const { setOpen } = useDialog()
  const errorId = useId()

  const { data, refetch } = useClashInfo()

  const { upsert } = useClashAccess()

  const runtimeProfile = useRuntimeProfile()

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      coreSecret: data?.secret || '',
    },
  })

  const handleSubmit = useLockFn(
    form.handleSubmit(
      async (data) => {
        try {
          await upsert.mutateAsync({
            secret: data.coreSecret,
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
      (error) => {
        message(formatError(error), {
          title: 'Error',
          kind: 'error',
          error,
        })
      },
    ),
  )

  const handleCopyClick = useLockFn(async () => {
    if (!data?.secret) {
      return
    }

    try {
      await writeClipboardText(data.secret)

      message(m.settings_clash_settings_core_secret_copied(), {
        title: 'Success',
        kind: 'info',
      })
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <>
      <DialogTrigger asChild>
        <ItemAction
          data-slot="core-secret-config-card"
          title={m.settings_clash_settings_core_secret_label()}
          description={data?.secret}
        />
      </DialogTrigger>

      <FormDialogContent
        title={m.settings_clash_settings_core_secret_label_edit()}
        submitLabel={m.common_apply()}
        cancelLabel={m.common_close()}
        submitting={form.formState.isSubmitting}
        extraActions={
          <Button
            type="button"
            variant="basic"
            onClick={handleCopyClick}
            disabled={!data?.secret || form.formState.isSubmitting}
          >
            <ContentCopyRounded className="size-5" aria-hidden />
            <span>{m.common_copy()}</span>
          </Button>
        }
        onOpen={() => form.reset({ coreSecret: data?.secret || '' })}
        onSubmit={handleSubmit}
      >
        <Controller
          control={form.control}
          name="coreSecret"
          render={({ field }) => {
            const error = form.formState.errors.coreSecret

            return (
              <>
                <Input
                  variant="underlined"
                  label={m.settings_clash_settings_core_secret_label()}
                  aria-label={m.settings_clash_settings_core_secret_label()}
                  value={field.value ?? ''}
                  onChange={(event) => field.onChange(event.target.value)}
                  aria-invalid={Boolean(error)}
                  aria-describedby={error ? errorId : undefined}
                />

                <FormDialogError id={errorId} message={error?.message} />
              </>
            )
          }}
        />
      </FormDialogContent>
    </>
  )
}
