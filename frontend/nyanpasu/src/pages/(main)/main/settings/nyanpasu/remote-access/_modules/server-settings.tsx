import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { useRemoteAccess } from '@nyanpasu/query'
import type { HttpAccessStrategy, HttpServerStatus } from '@nyanpasu/rpc/types'
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
import CertificateSettings from './certificate-settings'
import PermissionGrid from './permission-grid'
import { ANONYMOUS_LIMIT } from './permission-labels'
import SaveField from './save-field'
import SensitiveReach from './sensitive-reach'

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

const IPV4_PATTERN = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/

function isIpv4(text: string) {
  const match = IPV4_PATTERN.exec(text)

  return match != null && match.slice(1).every((part) => Number(part) <= 255)
}

function isIpv6(text: string) {
  if (!text.includes(':')) {
    return false
  }

  try {
    return new URL(`http://[${text}]/`).hostname !== ''
  } catch {
    return false
  }
}

/** The address to listen on, or null when the field is empty; undefined when it is no address. */
function parseLanBind(text: string): string | null | undefined {
  const trimmed = text.trim()
  if (trimmed === '') {
    return null
  }

  return isIpv4(trimmed) || isIpv6(trimmed) ? trimmed : undefined
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
    lan_authenticated: m.settings_remote_access_strategy_lan_authenticated(),
  } as Partial<Record<HttpAccessStrategy, string>>

  const descriptions = {
    local_authenticated:
      m.settings_remote_access_strategy_local_authenticated_description(),
    local_open: m.settings_remote_access_strategy_local_open_description(),
    lan_authenticated:
      m.settings_remote_access_strategy_lan_authenticated_description(),
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

  const faultMessages = {
    port_in_use: m.settings_remote_access_fault_port_in_use(),
    bind_failed: m.settings_remote_access_fault_bind_failed(),
    tls_unavailable: m.settings_remote_access_fault_tls_unavailable(),
  } satisfies Record<NonNullable<HttpServerStatus['fault']>, string>

  const loaded = http != null
  const locked = !loaded || update.isPending
  const strategy = http?.strategy ?? 'local_authenticated'
  const anonymousPermissions = http?.anonymous_permissions ?? {}

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
                {faultMessages[status.data.fault]}
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
              onChange={(next) =>
                // Credentials must not cross the network in the clear, so the LAN strategy
                // brings HTTPS with it.
                change(
                  next === 'lan_authenticated'
                    ? { strategy: next, tls: 'self_signed' }
                    : { strategy: next },
                )
              }
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
                  data-slot="remote-access-anonymous-permissions"
                >
                  <ItemLabel>
                    <ItemLabelText>
                      {m.settings_remote_access_anonymous_permissions_title()}
                    </ItemLabelText>

                    <ItemLabelDescription>
                      {m.settings_remote_access_anonymous_permissions_description()}
                    </ItemLabelDescription>
                  </ItemLabel>

                  <PermissionGrid
                    value={anonymousPermissions}
                    limit={ANONYMOUS_LIMIT}
                    disabled={locked}
                    onChange={(next) => change({ anonymous_permissions: next })}
                  />

                  <SensitiveReach held={anonymousPermissions} />
                </div>
              </>
            )}

            {strategy === 'lan_authenticated' && (
              <>
                <p
                  role="note"
                  data-slot="remote-access-lan-warning"
                  className="bg-error-container text-on-error-container rounded-xl px-4 py-3 text-sm"
                >
                  {m.settings_remote_access_strategy_lan_authenticated_warning()}
                </p>

                <SaveField
                  key={http?.lan_bind ?? 'all'}
                  slot="remote-access-lan-bind"
                  title={m.settings_remote_access_lan_bind_title()}
                  description={m.settings_remote_access_lan_bind_description()}
                  invalidMessage={m.settings_remote_access_lan_bind_invalid()}
                  committed={http?.lan_bind ?? null}
                  parse={parseLanBind}
                  disabled={locked}
                  onSave={(lan_bind) => change({ lan_bind })}
                />

                {status.data?.port != null && (
                  <span
                    data-slot="remote-access-lan-url-hint"
                    className="text-on-surface-variant text-sm select-text"
                  >
                    {m.settings_remote_access_lan_url_hint({
                      port: status.data.port,
                    })}
                  </span>
                )}
              </>
            )}
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_remote_access_tls_title()}
                </ItemLabelText>

                <ItemLabelDescription>
                  {m.settings_remote_access_tls_description()}
                </ItemLabelDescription>
              </ItemLabel>

              <Switch
                aria-label={m.settings_remote_access_tls_title()}
                checked={http?.tls === 'self_signed'}
                // The LAN strategy cannot run without HTTPS.
                disabled={locked || strategy === 'lan_authenticated'}
                onCheckedChange={(on) =>
                  change({ tls: on ? 'self_signed' : 'http' })
                }
              />
            </ItemContainer>

            {http?.tls === 'self_signed' && (
              <CertificateSettings
                tls={status.data?.tls ?? undefined}
                bindOutsideCertificate={
                  status.data?.bind_outside_certificate ?? false
                }
              />
            )}
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <SaveField
              key={http?.port ?? 'none'}
              slot="remote-access-port"
              title={m.settings_remote_access_port_title()}
              description={m.settings_remote_access_port_description()}
              invalidMessage={m.settings_remote_access_port_invalid()}
              inputMode="numeric"
              committed={http?.port ?? null}
              parse={parsePort}
              disabled={locked}
              onSave={(port) => change({ port })}
            />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}
