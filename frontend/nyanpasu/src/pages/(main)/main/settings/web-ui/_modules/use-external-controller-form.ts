import { useEffect } from 'react'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import { m } from '@/paraglide/messages'
import { zodResolver } from '@hookform/resolvers/zod'
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

  useEffect(() => {
    form.reset({
      externalController: configuredAddress,
    })
  }, [configuredAddress, form])

  const handleSubmit = form.handleSubmit(async (data) => {
    const address = parseControllerAddress(data.externalController)

    if (address) {
      await apply(address)
    }
  })

  return { form, handleSubmit }
}
