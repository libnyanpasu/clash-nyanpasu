import { useId } from 'react'
import { Controller } from 'react-hook-form'
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
import {
  formatControllerAddress,
  useClashInfo,
  useClashSystemSetting,
  useRuntimeProfile,
} from '@nyanpasu/query'
import { useExternalControllerForm } from './use-external-controller-form'

export default function ExternalControllerConfig() {
  return (
    <Dialog>
      <ExternalControllerConfigForm />
    </Dialog>
  )
}

function ExternalControllerConfigForm() {
  const { setOpen } = useDialog()
  const errorId = useId()

  const { data, refetch } = useClashInfo()

  const externalController = useClashSystemSetting('external_controller')

  // Edit the configured address, not the reported one: the reported server
  // turns an unspecified bind host into loopback and may carry a port the core
  // picked at runtime.
  const configuredAddress = externalController.value
    ? formatControllerAddress(externalController.value)
    : ''

  const runtimeProfile = useRuntimeProfile()

  const { form, handleSubmit } = useExternalControllerForm(
    configuredAddress,
    async (address) => {
      try {
        await externalController.upsert({
          host: address.host,
          port: { start_port: address.port },
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
  )

  return (
    <>
      <DialogTrigger asChild>
        <ItemAction
          data-slot="external-controller-config-card"
          title={m.settings_clash_settings_external_controll_label()}
          description={data?.server}
        />
      </DialogTrigger>

      <FormDialogContent
        title={m.settings_clash_settings_external_controll_label_edit()}
        submitLabel={m.common_apply()}
        cancelLabel={m.common_close()}
        submitting={form.formState.isSubmitting}
        onOpen={() => form.reset({ externalController: configuredAddress })}
        onSubmit={handleSubmit}
      >
        <Controller
          control={form.control}
          name="externalController"
          render={({ field }) => {
            const error = form.formState.errors.externalController

            return (
              <>
                <Input
                  variant="underlined"
                  label={m.settings_clash_settings_external_controll_label()}
                  aria-label={m.settings_clash_settings_external_controll_label()}
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
