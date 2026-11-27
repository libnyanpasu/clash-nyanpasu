import { useCallback, useId } from 'react'
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
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useSystemSetting } from '@nyanpasu/query'

const DEFAULT_BYPASS =
  'localhost;127.;192.168.;10.;' +
  '172.16.;172.17.;172.18.;172.19.;172.20.;172.21.;172.22.;172.23.;' +
  '172.24.;172.25.;172.26.;172.27.;172.28.;172.29.;172.30.;172.31.*'

const formSchema = z.object({
  systemProxyBypass: z.string().nullable().optional(),
})

export default function ProxyBypassConfig() {
  return (
    <Dialog>
      <ProxyBypassConfigForm />
    </Dialog>
  )
}

function ProxyBypassConfigForm() {
  const { setOpen } = useDialog()
  const errorId = useId()
  const systemProxyBypass = useSystemSetting('system_proxy_bypass')

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      systemProxyBypass: systemProxyBypass.value,
    },
  })

  const handleSubmit = form.handleSubmit(async (data) => {
    try {
      const next = data.systemProxyBypass || DEFAULT_BYPASS
      await systemProxyBypass.upsert(next)

      form.reset({ systemProxyBypass: next })
      setOpen(false)
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
      systemProxyBypass: systemProxyBypass.value,
    })
  }, [systemProxyBypass.value, form])

  const { isDirty, isSubmitting } = form.formState

  return (
    <>
      <DialogTrigger asChild>
        <ItemAction
          title={m.settings_system_proxy_proxy_bypass_label()}
          description={
            <span className="block truncate">
              {systemProxyBypass.value || DEFAULT_BYPASS}
            </span>
          }
        />
      </DialogTrigger>

      <FormDialogContent
        size="lg"
        title={m.settings_system_proxy_proxy_bypass_label()}
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
        onSubmit={handleSubmit}
      >
        <Controller
          control={form.control}
          name="systemProxyBypass"
          render={({ field }) => (
            <div className="flex flex-col gap-2">
              <Input
                variant="underlined"
                aria-label={m.settings_system_proxy_proxy_bypass_label()}
                aria-invalid={!!form.formState.errors.systemProxyBypass}
                aria-describedby={
                  form.formState.errors.systemProxyBypass ? errorId : undefined
                }
                value={field.value ?? ''}
                onChange={(event) => field.onChange(event.target.value)}
                autoFocus
              />

              <FormDialogError
                id={errorId}
                message={form.formState.errors.systemProxyBypass?.message}
              />
            </div>
          )}
        />
      </FormDialogContent>
    </>
  )
}
