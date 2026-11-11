import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input } from '@nyanpasu/ui/input'
import {
  ModalClose,
  ModalContent,
  ModalTitle,
  useModal,
} from '@nyanpasu/ui/modal'
import { reportCustomCssError } from '@/hooks/use-custom-css'
import { m } from '@/paraglide/messages'

/** The naming form inside the feature’s composed Modal root. */
export default function CustomCssNameDialog({
  title,
  initialName,
  onSubmit,
}: {
  title: string
  initialName: string
  onSubmit: (name: string) => Promise<void>
}) {
  const { setOpen } = useModal()
  const [saving, setSaving] = useState(false)
  const [name, setName] = useState(initialName)

  const trimmedName = name.trim()

  const handleSubmit = async () => {
    if (!trimmedName || saving) {
      return
    }

    setSaving(true)
    try {
      await onSubmit(trimmedName)
      setOpen(false)
    } catch (error) {
      await reportCustomCssError(error)
    } finally {
      setSaving(false)
    }
  }

  return (
    <ModalContent
      dismissible={!saving}
      onOpenAutoFocus={() => setName(initialName)}
    >
      <Card className="w-[28rem] max-w-full" data-slot="custom-css-name-dialog">
        <CardHeader>
          <ModalTitle>{title}</ModalTitle>
        </CardHeader>

        <CardContent>
          <form
            data-slot="custom-css-name-form"
            onSubmit={(event) => {
              event.preventDefault()
              handleSubmit()
            }}
          >
            <Input
              className="w-full"
              label={m.settings_user_interface_custom_css_name()}
              aria-label={m.settings_user_interface_custom_css_name()}
              value={name}
              autoFocus
              spellCheck={false}
              onChange={(event) => setName(event.target.value)}
            />
          </form>
        </CardContent>

        <CardFooter className="gap-1">
          <ModalClose disabled={saving}>{m.common_cancel()}</ModalClose>

          <Button
            variant="flat"
            loading={saving}
            disabled={!trimmedName || saving}
            onClick={handleSubmit}
          >
            {m.common_save()}
          </Button>
        </CardFooter>
      </Card>
    </ModalContent>
  )
}
