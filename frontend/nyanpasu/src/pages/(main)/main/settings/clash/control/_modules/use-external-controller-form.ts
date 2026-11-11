import type { BaseSyntheticEvent } from 'react'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import { m } from '@/paraglide/messages'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import { parseControllerAddress, type ControllerAddress } from '@nyanpasu/query'

const formSchema = z.object({
  externalController: z
    .string()
    .refine((value) => parseControllerAddress(value) !== null, {
      message: m.settings_clash_settings_external_controll_invalid_address(),
    }),
})

/**
 * The external controller editor's form, starting from `configuredAddress`.
 * The dialog resets it on opening; only one save can run at a time.
 * An address that does not parse only sets the inline field error:
 * `handleSubmit` has no invalid handler, so `apply` never sees it.
 */
export function useExternalControllerForm(
  configuredAddress: string,
  apply: (address: ControllerAddress) => Promise<void>,
) {
  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      externalController: configuredAddress,
    },
  })

  const submit = useLockFn(
    form.handleSubmit(async (data) => {
      const address = parseControllerAddress(data.externalController)

      if (address) {
        await apply(address)
      }
    }),
  )

  const handleSubmit = (event?: BaseSyntheticEvent) => {
    // Repeated Enter presses must suppress native submission even while locked.
    event?.preventDefault()
    return submit(event)
  }

  return { form, handleSubmit }
}
