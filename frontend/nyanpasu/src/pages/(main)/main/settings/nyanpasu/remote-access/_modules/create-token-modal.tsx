import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Checkbox } from '@nyanpasu/ui/checkbox'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { Input } from '@nyanpasu/ui/input'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@nyanpasu/ui/modal'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import type { HeldPermissions } from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { writeClipboardText } from '@/utils/clipboard'
import { useRemoteAccessTokens } from '@nyanpasu/query'
import { ASSIGNABLE_ROLES, ROLE_PERMISSIONS } from '@nyanpasu/rpc'
import type { CreatedRemoteAccessToken, Grant } from '@nyanpasu/rpc/types'
import GrantReach from './grant-reach'
import PermissionGrid from './permission-grid'
import { ROLE_LABELS } from './permission-labels'

const DAY_MS = 24 * 60 * 60 * 1000

const EXPIRIES = [
  { id: 'never', days: null },
  { id: '7_days', days: 7 },
  { id: '30_days', days: 30 },
  { id: '90_days', days: 90 },
] as const

type ExpiryId = (typeof EXPIRIES)[number]['id']

function expiryLabel(id: ExpiryId) {
  switch (id) {
    case 'never':
      return m.settings_remote_access_token_expiry_never()
    case '7_days':
      return m.settings_remote_access_token_expiry_7_days()
    case '30_days':
      return m.settings_remote_access_token_expiry_30_days()
    case '90_days':
      return m.settings_remote_access_token_expiry_90_days()
  }
}

/** A role, or `custom` for a hand-picked permission map. */
type GrantChoice = (typeof ASSIGNABLE_ROLES)[number] | 'custom'

const DEFAULT_CHOICE: GrantChoice = 'viewer'

/** What each choice is for; the grid below it shows exactly what it grants. */
const GRANT_DESCRIPTIONS = {
  viewer: m.settings_remote_access_role_viewer_description,
  operator: m.settings_remote_access_role_operator_description,
  admin: m.settings_remote_access_role_admin_description,
  custom: m.settings_remote_access_role_custom_description,
} satisfies Record<GrantChoice, () => string>

/** The form, then the one-time token; closing is blocked until the token is confirmed saved. */
function CreateTokenDialog() {
  const { setOpen } = useModal()
  const { create } = useRemoteAccessTokens()

  const [name, setName] = useState('')
  const [choice, setChoice] = useState<GrantChoice>(DEFAULT_CHOICE)
  const [custom, setCustom] = useState<HeldPermissions>({})
  const [expiry, setExpiry] = useState<ExpiryId>('never')
  const [created, setCreated] = useState<CreatedRemoteAccessToken | null>(null)
  const [confirmed, setConfirmed] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const trimmedName = name.trim()
  const saving = create.isPending
  const grant: Grant =
    choice === 'custom' ? { permissions: custom } : { role: choice }
  const held: HeldPermissions =
    choice === 'custom' ? custom : ROLE_PERMISSIONS[choice]
  const canCreate =
    trimmedName !== '' && Object.keys(held).length > 0 && !saving
  const mustConfirm = created != null && !confirmed

  const reset = () => {
    setName('')
    setChoice(DEFAULT_CHOICE)
    setCustom({})
    setExpiry('never')
    setCreated(null)
    setConfirmed(false)
    setError(null)
    create.reset()
  }

  const handleCreate = async () => {
    if (!canCreate) {
      return
    }

    const days = EXPIRIES.find((option) => option.id === expiry)?.days ?? null
    setError(null)
    try {
      setCreated(
        await create.mutateAsync({
          name: trimmedName,
          grant,
          expires_at_ms: days == null ? null : Date.now() + days * DAY_MS,
        }),
      )
    } catch (failure) {
      setError(formatError(failure))
    }
  }

  return (
    <ModalContent
      dismissible={!saving && !mustConfirm}
      onOpenAutoFocus={reset}
      onExitComplete={reset}
    >
      <Card
        className="flex max-h-[calc(100dvh-4rem)] w-[28rem] max-w-full flex-col"
        data-slot="remote-access-create-token-dialog"
      >
        {created ? (
          <>
            <CardHeader>
              <ModalTitle>
                {m.settings_remote_access_token_created_title()}
              </ModalTitle>
            </CardHeader>

            <CardContent>
              <p className="text-sm">
                {m.settings_remote_access_token_created_warning()}
              </p>

              <code
                className="bg-surface-container-high rounded-medium p-3 text-sm break-all select-text"
                data-slot="remote-access-created-token"
              >
                {created.token}
              </code>

              <div>
                <Button
                  variant="stroked"
                  onClick={() => writeClipboardText(created.token)}
                >
                  {m.settings_remote_access_token_copy()}
                </Button>
              </div>

              <label className="flex cursor-pointer items-center gap-3 text-sm">
                <Checkbox
                  checked={confirmed}
                  onCheckedChange={(checked) => setConfirmed(checked === true)}
                />

                {m.settings_remote_access_token_confirm()}
              </label>
            </CardContent>

            <CardFooter className="gap-1">
              <Button
                variant="flat"
                disabled={!confirmed}
                onClick={() => setOpen(false)}
              >
                {m.settings_remote_access_token_done()}
              </Button>
            </CardFooter>
          </>
        ) : (
          <>
            <CardHeader>
              <ModalTitle>
                {m.settings_remote_access_token_create_title()}
              </ModalTitle>
            </CardHeader>

            <CardContent className="min-h-0 overflow-y-auto">
              <form
                className="flex flex-col gap-4"
                onSubmit={(event) => {
                  event.preventDefault()
                  handleCreate()
                }}
              >
                <Input
                  className="w-full"
                  label={m.settings_remote_access_token_name()}
                  aria-label={m.settings_remote_access_token_name()}
                  value={name}
                  autoFocus
                  spellCheck={false}
                  onChange={(event) => setName(event.target.value)}
                />

                <fieldset className="flex flex-col gap-3">
                  <legend className="mb-2 text-sm font-medium">
                    {m.settings_remote_access_token_grant()}
                  </legend>

                  <SegmentedButton
                    size="sm"
                    aria-label={m.settings_remote_access_token_grant()}
                    value={choice}
                    onValueChange={(next) =>
                      next && setChoice(next as GrantChoice)
                    }
                  >
                    {ASSIGNABLE_ROLES.map((role) => (
                      <SegmentedButtonItem key={role} value={role}>
                        {ROLE_LABELS[role]()}
                      </SegmentedButtonItem>
                    ))}

                    <SegmentedButtonItem value="custom">
                      {m.settings_remote_access_role_custom()}
                    </SegmentedButtonItem>
                  </SegmentedButton>

                  <p
                    className="text-on-surface-variant text-sm"
                    data-slot="remote-access-token-grant-description"
                  >
                    {GRANT_DESCRIPTIONS[choice]()}
                  </p>

                  {choice === 'custom' ? (
                    <PermissionGrid value={custom} onChange={setCustom} />
                  ) : (
                    <PermissionGrid value={held} />
                  )}

                  <GrantReach held={held} />
                </fieldset>

                <DropdownMenu align="start">
                  <DropdownMenuTrigger asChild>
                    <Button variant="stroked" className="justify-start">
                      {m.settings_remote_access_token_expiry()}:{' '}
                      {expiryLabel(expiry)}
                    </Button>
                  </DropdownMenuTrigger>

                  <DropdownMenuContent>
                    {EXPIRIES.map((option) => (
                      <DropdownMenuCheckboxItem
                        key={option.id}
                        checked={expiry === option.id}
                        onSelect={() => setExpiry(option.id)}
                      >
                        {expiryLabel(option.id)}
                      </DropdownMenuCheckboxItem>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>

                {error && (
                  <p role="alert" className="text-error text-sm">
                    {error}
                  </p>
                )}
              </form>
            </CardContent>

            <CardFooter className="gap-1">
              <ModalClose disabled={saving}>{m.common_cancel()}</ModalClose>

              <Button
                variant="flat"
                loading={saving}
                disabled={!canCreate}
                onClick={handleCreate}
              >
                {m.settings_remote_access_token_create()}
              </Button>
            </CardFooter>
          </>
        )}
      </Card>
    </ModalContent>
  )
}

export default function CreateTokenModal() {
  return (
    <Modal>
      <ModalTrigger asChild>
        <Button variant="flat" data-slot="remote-access-create-token">
          {m.settings_remote_access_token_create()}
        </Button>
      </ModalTrigger>

      <CreateTokenDialog />
    </Modal>
  )
}
