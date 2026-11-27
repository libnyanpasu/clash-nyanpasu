import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { useId, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { FormDialogContent, FormDialogError } from '@nyanpasu/ui/dialog'
import { NumericInput } from '@nyanpasu/ui/input'
import { Modal, ModalTrigger, useModal } from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'

const PORT_MAX = 65535

/** One outlined port field; an empty field leaves the choice to the server's first start. */
function PortDialog({
  port,
  onApply,
}: {
  port: number | null
  onApply: (port: number | null) => Promise<unknown>
}) {
  const { setOpen } = useModal()
  const errorId = useId()
  const [draft, setDraft] = useState<number | null>(port)
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)

  const apply = async () => {
    setError(null)
    setSaving(true)
    try {
      await onApply(draft)
      setOpen(false)
    } catch (failure) {
      setError(formatError(failure))
    } finally {
      setSaving(false)
    }
  }

  return (
    <FormDialogContent
      title={m.settings_remote_access_port_title()}
      submitLabel={m.common_apply()}
      cancelLabel={m.common_close()}
      submitting={saving}
      onOpen={() => {
        setDraft(port)
        setError(null)
      }}
      onSubmit={apply}
    >
      <div
        className="flex flex-col gap-4"
        data-slot="remote-access-port-dialog"
      >
        <NumericInput
          variant="outlined"
          label={m.settings_remote_access_port_title()}
          aria-label={m.settings_remote_access_port_title()}
          value={draft}
          onChange={setDraft}
          min={1}
          max={PORT_MAX}
          allowNegative={false}
          decimalScale={0}
          aria-invalid={Boolean(error)}
          aria-describedby={error ? errorId : undefined}
        />

        <p className="text-on-surface-variant text-sm">
          {m.settings_remote_access_port_description()}
        </p>

        <FormDialogError id={errorId} message={error} />
      </div>
    </FormDialogContent>
  )
}

/**
 * The listening port as a row, like the core's port settings: it shows the saved port and opens
 * a dialog with one outlined field.
 */
export default function PortSettings({
  port,
  disabled,
  onApply,
}: {
  port: number | null
  disabled: boolean
  onApply: (port: number | null) => Promise<unknown>
}) {
  return (
    <SettingsCard data-slot="remote-access-port">
      <Modal>
        <SettingsCardContent asChild>
          <ModalTrigger asChild disabled={disabled}>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_remote_access_port_title()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {port ?? m.settings_remote_access_port_auto()}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </ModalTrigger>
        </SettingsCardContent>

        <PortDialog port={port} onApply={onApply} />
      </Modal>
    </SettingsCard>
  )
}
