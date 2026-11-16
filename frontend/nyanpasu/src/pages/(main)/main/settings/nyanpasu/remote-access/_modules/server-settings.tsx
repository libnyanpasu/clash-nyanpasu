import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { Input } from '@nyanpasu/ui/input'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { useRemoteAccess } from '@nyanpasu/query'
import type { HttpAccessStrategy, Scope } from '@nyanpasu/rpc/types'
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
import ScopeCheckboxes from './scope-checkboxes'
import { LOCAL_OPEN_SCOPES } from './scope-labels'

const DEFAULT_LOCAL_OPEN_SCOPES: Scope[] = ['read']

const PORT_MAX = 65535

/** The persisted port, or null when the field is empty; undefined when it is not a port. */
function parsePort(text: string): number | null | undefined {
  const trimmed = text.trim()
  if (trimmed === '') {
    return null
  }

  const port = Number(trimmed)
  return Number.isInteger(port) && port >= 1 && port <= PORT_MAX
    ? port
    : undefined
}

function PortField({
  committed,
  disabled,
  onSave,
}: {
  committed: number | null
  disabled: boolean
  onSave: (port: number | null) => void
}) {
  const [text, setText] = useState(committed?.toString() ?? '')

  const parsed = parsePort(text)
  const invalid = parsed === undefined
  const unchanged = parsed === committed

  return (
    <div className="flex flex-col gap-2" data-slot="remote-access-port">
      <ItemLabel>
        <ItemLabelText>{m.settings_remote_access_port_title()}</ItemLabelText>

        <ItemLabelDescription>
          {m.settings_remote_access_port_description()}
        </ItemLabelDescription>
      </ItemLabel>

      <form
        className="flex items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault()
          if (!invalid && !unchanged) {
            onSave(parsed)
          }
        }}
      >
        <Input
          className="w-full"
          label={m.settings_remote_access_port_title()}
          aria-label={m.settings_remote_access_port_title()}
          inputMode="numeric"
          value={text}
          disabled={disabled}
          onChange={(event) => setText(event.target.value)}
        />

        <Button
          type="submit"
          variant="flat"
          disabled={disabled || invalid || unchanged}
        >
          {m.common_save()}
        </Button>
      </form>

      {invalid && (
        <p role="alert" className="text-error text-sm">
          {m.settings_remote_access_port_invalid()}
        </p>
      )}
    </div>
  )
}

function StrategySelector({
  value,
  disabled,
  onChange,
}: {
  value: HttpAccessStrategy
  disabled: boolean
  onChange: (strategy: HttpAccessStrategy) => void
}) {
  const messages = {
    local_authenticated:
      m.settings_remote_access_strategy_local_authenticated(),
    local_open: m.settings_remote_access_strategy_local_open(),
  } as Partial<Record<HttpAccessStrategy, string>>

  const descriptions = {
    local_authenticated:
      m.settings_remote_access_strategy_local_authenticated_description(),
    local_open: m.settings_remote_access_strategy_local_open_description(),
  } as Partial<Record<HttpAccessStrategy, string>>

  return (
    <DropdownMenu align="end">
      <DropdownMenuTrigger asChild disabled={disabled}>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-0 text-left text-base"
          data-slot="remote-access-strategy-trigger"
        >
          <ItemContainer className="w-full">
            <ItemLabel>
              <ItemLabelText>
                {m.settings_remote_access_strategy_title()}
              </ItemLabelText>

              <ItemLabelDescription>
                {messages[value] ?? value}
              </ItemLabelDescription>
            </ItemLabel>

            <ArrowForwardIosRounded />
          </ItemContainer>
        </Button>
      </DropdownMenuTrigger>

      <DropdownMenuContent>
        {Object.entries(messages).map(([key, message]) => (
          <DropdownMenuCheckboxItem
            key={key}
            checked={value === key}
            onSelect={() => onChange(key as HttpAccessStrategy)}
          >
            <span className="flex flex-col">
              <span>{message}</span>

              <span className="text-on-surface-variant text-xs">
                {descriptions[key as HttpAccessStrategy]}
              </span>
            </span>
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** The listener's switch, web interface, sign-in strategy and port, and what is listening now. */
export default function ServerSettings() {
  const { status, http, update, openInBrowser } = useRemoteAccess()
  const [error, setError] = useState<string | null>(null)

  const reportFailure = (value: unknown) => setError(formatError(value))

  const change = (patch: Parameters<typeof update.mutate>[0]) => {
    setError(null)
    update.mutate(patch, { onError: reportFailure })
  }

  const loaded = http != null
  const locked = !loaded || update.isPending
  const strategy = http?.strategy ?? 'local_authenticated'
  const localOpenScopes = http?.local_open_scopes ?? DEFAULT_LOCAL_OPEN_SCOPES

  return (
    <div data-slot="remote-access-server-container">
      <SettingsLabel>{m.settings_remote_access_http_label()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_remote_access_enabled_title()}
                </ItemLabelText>

                <ItemLabelDescription>
                  {m.settings_remote_access_enabled_description()}
                </ItemLabelDescription>
              </ItemLabel>

              <Switch
                aria-label={m.settings_remote_access_enabled_title()}
                // The committed setting: a listener that failed to bind leaves it on, and the
                // user can still turn it off.
                checked={http?.enabled ?? false}
                disabled={locked}
                onCheckedChange={(enabled) => change({ enabled })}
              />
            </ItemContainer>

            {status.data?.url && (
              <span data-slot="remote-access-listening" className="select-text">
                {m.settings_remote_access_listening({ url: status.data.url })}
              </span>
            )}

            {status.data?.enabled && (
              <div>
                <Button
                  data-slot="remote-access-open"
                  disabled={openInBrowser.isPending}
                  onClick={() => {
                    setError(null)
                    openInBrowser.mutate(undefined, { onError: reportFailure })
                  }}
                >
                  {m.settings_remote_access_open()}
                </Button>
              </div>
            )}

            {status.data?.fault && (
              <p
                role="alert"
                data-slot="remote-access-fault"
                className="text-error text-sm"
              >
                {status.data.fault === 'port_in_use'
                  ? m.settings_remote_access_fault_port_in_use()
                  : m.settings_remote_access_fault_bind_failed()}
              </p>
            )}

            {(error || status.isError) && (
              <p
                role="alert"
                data-slot="remote-access-error"
                className="text-error text-sm"
              >
                {error ?? m.settings_remote_access_status_error()}
              </p>
            )}
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_remote_access_serve_webui_title()}
                </ItemLabelText>

                <ItemLabelDescription>
                  {m.settings_remote_access_serve_webui_description()}
                </ItemLabelDescription>
              </ItemLabel>

              <Switch
                aria-label={m.settings_remote_access_serve_webui_title()}
                checked={http?.serve_webui ?? true}
                disabled={locked}
                onCheckedChange={(serve_webui) => change({ serve_webui })}
              />
            </ItemContainer>
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <StrategySelector
              value={strategy}
              disabled={locked}
              onChange={(next) => change({ strategy: next })}
            />

            {strategy === 'local_open' && (
              <>
                <p
                  role="note"
                  data-slot="remote-access-local-open-warning"
                  className="bg-error-container text-on-error-container rounded-xl px-4 py-3 text-sm"
                >
                  {m.settings_remote_access_strategy_local_open_warning()}
                </p>

                <div
                  className="flex flex-col gap-3"
                  data-slot="remote-access-local-open-scopes"
                >
                  <ItemLabel>
                    <ItemLabelText>
                      {m.settings_remote_access_local_open_scopes_title()}
                    </ItemLabelText>

                    <ItemLabelDescription>
                      {m.settings_remote_access_local_open_scopes_description()}
                    </ItemLabelDescription>
                  </ItemLabel>

                  <ScopeCheckboxes
                    scopes={LOCAL_OPEN_SCOPES}
                    value={localOpenScopes}
                    disabled={locked}
                    onChange={(next) => {
                      // An anonymous caller with no scope admits nothing, so the last one stays.
                      if (next.length > 0) {
                        change({ local_open_scopes: next })
                      }
                    }}
                  />
                </div>
              </>
            )}
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <PortField
              key={http?.port ?? 'none'}
              committed={http?.port ?? null}
              disabled={locked}
              onSave={(port) => change({ port })}
            />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}
