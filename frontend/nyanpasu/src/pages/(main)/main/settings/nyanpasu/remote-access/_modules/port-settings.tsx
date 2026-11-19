import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { NumericInput } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@nyanpasu/ui/modal'
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
    <ModalContent
      dismissible={!saving}
      onOpenAutoFocus={() => {
        setDraft(port)
        setError(null)
      }}
    >
      <Card
        className="flex min-w-96 flex-col"
        data-slot="remote-access-port-dialog"
      >
        <CardHeader>
          <ModalTitle>{m.settings_remote_access_port_title()}</ModalTitle>
        </CardHeader>

        <CardContent asChild>
          <form
            className="flex flex-col gap-2"
            onSubmit={(event) => {
              event.preventDefault()
              apply()
            }}
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
            />

            <p className="text-on-surface-variant px-4 text-sm">
              {m.settings_remote_access_port_description()}
            </p>

            {error && (
              <p role="alert" className="text-error px-4 text-sm">
                {error}
              </p>
            )}
          </form>
        </CardContent>

        <CardFooter className="gap-2">
          <Button
            variant="flat"
            loading={saving}
            disabled={saving}
            onClick={apply}
          >
            {m.common_apply()}
          </Button>

          <ModalClose disabled={saving}>{m.common_close()}</ModalClose>
        </CardFooter>
      </Card>
    </ModalContent>
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
