import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { formatDate } from '@/utils/date'
import { ask, message } from '@/utils/notification'
import { useRemoteAccessTokens } from '@nyanpasu/query'
import type { AccessTokenSummary } from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import CreateTokenModal from './create-token-modal'
import { scopeLabel } from './scope-labels'

function TokenRow({
  token,
  onRevoke,
}: {
  token: AccessTokenSummary
  onRevoke: (token: AccessTokenSummary) => void
}) {
  return (
    <li
      className="flex items-center justify-between gap-4"
      data-slot="remote-access-token"
    >
      <ItemLabel>
        <ItemLabelText>
          {token.name}{' '}
          <span className="text-on-surface-variant text-sm font-normal">
            {token.status === 'expired'
              ? m.settings_remote_access_token_status_expired()
              : m.settings_remote_access_token_status_active()}
          </span>
        </ItemLabelText>

        <ItemLabelDescription>
          {token.scopes.map(scopeLabel).join(' · ')}
        </ItemLabelDescription>

        <ItemLabelDescription>
          {m.settings_remote_access_token_created_at({
            date: formatDate(token.created_at_ms),
          })}
          {' · '}
          {token.last_used_at_ms == null
            ? m.settings_remote_access_token_never_used()
            : m.settings_remote_access_token_last_used({
                date: formatDate(token.last_used_at_ms),
              })}
          {token.expires_at_ms != null &&
            ` · ${m.settings_remote_access_token_expires_at({
              date: formatDate(token.expires_at_ms),
            })}`}
        </ItemLabelDescription>
      </ItemLabel>

      <Button variant="stroked" onClick={() => onRevoke(token)}>
        {m.settings_remote_access_token_revoke()}
      </Button>
    </li>
  )
}

export default function AccessTokens() {
  const { query, revoke } = useRemoteAccessTokens()

  const handleRevoke = async (token: AccessTokenSummary) => {
    const confirmed = await ask(
      m.settings_remote_access_token_revoke_confirm({ name: token.name }),
      { kind: 'warning' },
    )
    if (!confirmed) {
      return
    }

    try {
      await revoke.mutateAsync(token.id)
    } catch (error) {
      await message(formatError(error), { kind: 'error', error })
    }
  }

  return (
    <div data-slot="remote-access-tokens-container">
      <SettingsLabel>{m.settings_remote_access_tokens_title()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelDescription>
                  {m.settings_remote_access_tokens_description()}
                </ItemLabelDescription>
              </ItemLabel>

              <CreateTokenModal />
            </ItemContainer>

            {query.isError ? (
              <p role="alert" className="text-error text-sm">
                {m.settings_remote_access_tokens_error()}
              </p>
            ) : query.data?.length === 0 ? (
              <p className="text-on-surface-variant text-sm">
                {m.settings_remote_access_tokens_empty()}
              </p>
            ) : (
              <ul className="flex flex-col gap-4">
                {query.data?.map((token) => (
                  <TokenRow
                    key={token.id}
                    token={token}
                    onRevoke={handleRevoke}
                  />
                ))}
              </ul>
            )}
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}
