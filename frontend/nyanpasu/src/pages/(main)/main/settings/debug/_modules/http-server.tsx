import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { useMatchesPlatform } from '@nyanpasu/platform/react'
import { useHttpServer } from '@nyanpasu/query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../../_modules/settings-card'

export default function HttpServer() {
  const desktop = useMatchesPlatform('desktop')
  const { status, enabled, setEnabled, openInBrowser } = useHttpServer()
  const [error, setError] = useState<string | null>(null)

  const reportFailure = (value: unknown) =>
    setError(
      typeof value === 'object' && value !== null && 'message' in value
        ? String(value.message)
        : String(value),
    )

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardContent data-slot="http-server-container">
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>
                {m.settings_debug_http_server_title()}
              </ItemLabelText>
              <ItemLabelDescription>
                {desktop
                  ? m.settings_debug_http_server_description_desktop()
                  : m.settings_debug_http_server_description_browser()}
              </ItemLabelDescription>
            </ItemLabel>
            <Switch
              aria-label={m.settings_debug_http_server_title()}
              // The committed setting: a listener that failed to bind leaves it on, and the
              // user can still turn it off.
              checked={enabled ?? false}
              disabled={!desktop || enabled == null || setEnabled.isPending}
              onCheckedChange={(next) => {
                setError(null)
                setEnabled.mutate(next, { onError: reportFailure })
              }}
            />
          </ItemContainer>

          {status.data?.url && (
            <span data-slot="http-server-listening" className="select-text">
              {m.settings_debug_http_server_listening({ url: status.data.url })}
            </span>
          )}

          {desktop && status.data?.enabled && (
            <Button
              data-slot="http-server-open"
              disabled={openInBrowser.isPending}
              onClick={() => {
                setError(null)
                openInBrowser.mutate(undefined, { onError: reportFailure })
              }}
            >
              {m.settings_debug_http_server_open()}
            </Button>
          )}

          {status.data?.fault && (
            <p role="alert" data-slot="http-server-fault">
              {status.data.fault === 'port_in_use'
                ? m.settings_debug_http_server_fault_port_in_use()
                : m.settings_debug_http_server_fault_bind_failed()}
            </p>
          )}

          {(error || status.isError) && (
            <p role="alert" data-slot="http-server-error">
              {error ?? m.settings_debug_http_server_status_error()}
            </p>
          )}
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
