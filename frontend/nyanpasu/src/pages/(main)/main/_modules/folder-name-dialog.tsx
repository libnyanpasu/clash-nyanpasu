import { AnimatePresence } from 'motion/react'
import { useEffect } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input } from '@nyanpasu/ui/input'
import { Modal, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { zodResolver } from '@hookform/resolvers/zod'
import { useLockFn } from '@nyanpasu/hooks'
import AnimatedErrorItem from './error-item'

const formSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, { error: () => m.profile_folder_name_required() }),
})

type FormValues = z.infer<typeof formSchema>

/** Asks for a folder name; `onSubmit` receives the trimmed, non-empty name. */
export default function FolderNameDialog({
  open,
  onOpenChange,
  title,
  initialName = '',
  onSubmit,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  initialName?: string
  onSubmit: (name: string) => Promise<void>
}) {
  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: { name: initialName },
  })

  useEffect(() => {
    if (open) {
      form.reset({ name: initialName })
    }
  }, [open, initialName, form])

  const handleOpenChange = (value: boolean) => {
    if (form.formState.isSubmitting) {
      return
    }

    onOpenChange(value)
  }

  const handleSubmit = useLockFn(
    form.handleSubmit(async ({ name }) => {
      await onSubmit(name)
    }),
  )

  return (
    <Modal open={open} onOpenChange={handleOpenChange}>
      <ModalContent>
        <Card className="w-96" data-slot="folder-name-dialog">
          <CardHeader>
            <ModalTitle>{title}</ModalTitle>
          </CardHeader>

          <CardContent>
            <form
              id="folder-name-dialog-form"
              onSubmit={(event) => {
                event.preventDefault()
                handleSubmit()
              }}
            >
              <Controller
                control={form.control}
                name="name"
                render={({ field }) => (
                  <div className="space-y-2">
                    <Input
                      label={m.profile_folder_name_label()}
                      aria-label={m.profile_folder_name_label()}
                      variant="outlined"
                      autoFocus
                      {...field}
                    />

                    <AnimatePresence>
                      {form.formState.errors.name && (
                        <AnimatedErrorItem className="text-error">
                          {form.formState.errors.name.message}
                        </AnimatedErrorItem>
                      )}
                    </AnimatePresence>
                  </div>
                )}
              />
            </form>
          </CardContent>

          <CardFooter className="gap-1">
            <Button
              type="submit"
              form="folder-name-dialog-form"
              loading={form.formState.isSubmitting}
            >
              {m.common_save()}
            </Button>

            <Button onClick={() => handleOpenChange(false)}>
              {m.common_cancel()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}
