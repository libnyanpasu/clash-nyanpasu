import { useState } from 'react'
import { FormDialogContent } from '@nyanpasu/ui/dialog'
import { Input } from '@nyanpasu/ui/input'
import { useModal } from '@nyanpasu/ui/modal'
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
    <FormDialogContent
      title={title}
      submitLabel={m.common_save()}
      cancelLabel={m.common_cancel()}
      submitting={saving}
      submitDisabled={!trimmedName}
      onOpen={() => setName(initialName)}
      onSubmit={handleSubmit}
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
    </FormDialogContent>
  )
}
