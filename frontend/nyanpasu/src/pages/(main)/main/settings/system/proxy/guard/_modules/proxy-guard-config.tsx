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
import { NumericInput } from '@nyanpasu/ui/input'
import { ItemAction } from '@nyanpasu/ui/item-group'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useSystemSetting } from '@nyanpasu/query'

const formSchema = z.object({
  proxyGuardInterval: z.number().min(1),
})

export default function ProxyGuardConfig() {
  return (
    <Dialog>
      <ProxyGuardConfigForm />
    </Dialog>
  )
}

function ProxyGuardConfigForm() {
  const { setOpen } = useDialog()
  const errorId = useId()
  const proxyGuardInterval = useSystemSetting('proxy_guard_interval')

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      proxyGuardInterval: proxyGuardInterval.value || 1,
    },
  })

  const handleSubmit = form.handleSubmit(async (data) => {
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
  })

  const handleReset = () => {
    form.reset({
      proxyGuardInterval: proxyGuardInterval.value || 1,
    })
  }

  const { isDirty, isSubmitting } = form.formState

  return (
    <>
      <DialogTrigger asChild>
        <ItemAction
          title={m.settings_system_proxy_proxy_guard_interval_label()}
          description={String(proxyGuardInterval.value || 1)}
        />
      </DialogTrigger>

      <FormDialogContent
        title={m.settings_system_proxy_proxy_guard_interval_label()}
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
          name="proxyGuardInterval"
          control={form.control}
          render={({ field }) => (
            <div className="flex flex-col gap-2" data-slot="proxy-guard-form">
              <NumericInput
                value={field.value}
                disabled={isSubmitting}
                aria-label={m.settings_system_proxy_proxy_guard_interval_label()}
                aria-invalid={!!form.formState.errors.proxyGuardInterval}
                aria-describedby={
                  form.formState.errors.proxyGuardInterval ? errorId : undefined
                }
                onChange={(value) => field.onChange(value)}
                autoFocus
              />

              <FormDialogError
                id={errorId}
                message={form.formState.errors.proxyGuardInterval?.message}
              />
            </div>
          )}
        />
      </FormDialogContent>
    </>
  )
}
