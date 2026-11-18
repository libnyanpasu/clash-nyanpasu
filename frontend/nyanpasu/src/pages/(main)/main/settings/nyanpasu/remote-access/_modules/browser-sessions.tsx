import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { formatDate } from '@/utils/date'
import { ask, message } from '@/utils/notification'
import {
  useRemoteAccessTokens,
  useRemoteBrowserSessions,
} from '@nyanpasu/query'
import type { BrowserSessionSummary } from '@nyanpasu/rpc/types'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import { grantLabel } from './permission-labels'

export default function BrowserSessions() {
  const { query, revoke } = useRemoteBrowserSessions()
  const tokens = useRemoteAccessTokens().query

  const tokenName = (session: BrowserSessionSummary) => {
    if (session.backing_token_id == null) {
      return m.settings_remote_access_session_via_launch()
    }

    const token = tokens.data?.find((t) => t.id === session.backing_token_id)
    return m.settings_remote_access_session_via_token({
      name: token?.name ?? session.backing_token_id,
    })
  }

  const handleRevoke = async (session: BrowserSessionSummary) => {
    const confirmed = await ask(
      m.settings_remote_access_session_revoke_confirm(),
      {
        kind: 'warning',
      },
    )
    if (!confirmed) {
      return
    }

    try {
      await revoke.mutateAsync(session.id)
    } catch (error) {
      await message(formatError(error), { kind: 'error', error })
    }
  }

  return (
    <div data-slot="remote-access-sessions-container">
      <SettingsLabel>{m.settings_remote_access_sessions_title()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <ItemLabelDescription>
              {m.settings_remote_access_sessions_description()}
            </ItemLabelDescription>

            {query.isError ? (
              <p role="alert" className="text-error text-sm">
                {m.settings_remote_access_sessions_error()}
              </p>
            ) : query.data?.length === 0 ? (
              <p className="text-on-surface-variant text-sm">
                {m.settings_remote_access_sessions_empty()}
              </p>
            ) : (
              <ul className="flex flex-col gap-4">
                {query.data?.map((session) => (
                  <li
                    key={session.id}
                    className="flex items-center justify-between gap-4"
                    data-slot="remote-access-session"
                  >
                    <ItemLabel>
                      <ItemLabelText>{tokenName(session)}</ItemLabelText>

                      <ItemLabelDescription>
                        {grantLabel(session.grant)}
                      </ItemLabelDescription>

                      <ItemLabelDescription>
                        {m.settings_remote_access_session_last_active({
                          date: formatDate(session.last_active_at_ms),
                        })}
                      </ItemLabelDescription>
                    </ItemLabel>

                    <Button
                      variant="stroked"
                      onClick={() => handleRevoke(session)}
                    >
                      {m.settings_remote_access_session_revoke()}
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}
