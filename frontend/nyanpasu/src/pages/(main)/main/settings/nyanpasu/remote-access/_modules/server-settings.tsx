import { useState } from 'react'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { useLocalNetworks, useRemoteAccess } from '@nyanpasu/query'
import { ANONYMOUS_PERMISSIONS_LIMIT } from '@nyanpasu/rpc'
import type {
  HttpAuthentication,
  HttpListen,
  HttpServerStatus,
} from '@nyanpasu/rpc/types'
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
import AccessAddresses from './access-addresses'
import CertificateSettings from './certificate-settings'
import GrantReach from './grant-reach'
import LanAllowList, { defaultLanAllow } from './lan-allow-list'
import PermissionGrid from './permission-grid'
import PortSettings from './port-settings'

const LISTEN_SCOPES = ['local', 'lan', 'any'] as const satisfies HttpListen[]

const AUTH_MODES = [
  'required',
  'optional',
] as const satisfies HttpAuthentication[]

/**
 * The addresses of the running server, then the listener's switch and web interface, followed by
 * the settings that decide together who can connect and how: the port, HTTPS, the reach and the
 * authentication, which leads into the access tokens below.
 */
export default function ServerSettings() {
  const { status, http, update } = useRemoteAccess()
  const networks = useLocalNetworks({ enabled: http != null })
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

  const listenLabels = {
    local: m.settings_remote_access_listen_local(),
    lan: m.settings_remote_access_listen_lan(),
    any: m.settings_remote_access_listen_any(),
  } satisfies Record<HttpListen, string>

  const authLabels = {
    required: m.settings_remote_access_auth_required(),
    optional: m.settings_remote_access_auth_optional(),
  } satisfies Record<HttpAuthentication, string>

  const loaded = http != null
  const locked = !loaded || update.isPending
  const listen = http?.listen ?? 'local'
  const auth = http?.auth ?? 'required'
  const lanAllow = http?.lan_allow ?? []
  const anonymousPermissions = http?.anonymous_permissions ?? {}
  // Nothing to allow yet, so the LAN scope would be refused.
  const noLanNetwork =
    lanAllow.length === 0 && networks.isSuccess && networks.data.length === 0

  const chooseListen = (next: HttpListen) => {
    if (next === 'local') {
      change({ listen: next })
      return
    }

    // Credentials must not cross the network in the clear, and only this computer may skip them,
    // so a network scope brings HTTPS and authentication with it.
    change({
      listen: next,
      tls: 'self_signed',
      auth: 'required',
      ...(next === 'lan' && lanAllow.length === 0
        ? { lan_allow: defaultLanAllow(networks.data ?? []) }
        : {}),
    })
  }

  return (
    <div data-slot="remote-access-server-container">
      <AccessAddresses />

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

        <PortSettings
          port={http?.port ?? null}
          disabled={locked}
          onApply={(port) => update.mutateAsync({ port })}
        />

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
                // A network scope cannot run without HTTPS.
                disabled={locked || listen !== 'local'}
                onCheckedChange={(on) =>
                  change({ tls: on ? 'self_signed' : 'http' })
                }
              />
            </ItemContainer>

            {http?.tls === 'self_signed' && <CertificateSettings />}
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <div
              className="flex flex-col gap-3"
              data-slot="remote-access-listen"
            >
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_remote_access_listen_title()}
                </ItemLabelText>

                <ItemLabelDescription>
                  {m.settings_remote_access_listen_description()}
                </ItemLabelDescription>
              </ItemLabel>

              <SegmentedButton
                className="w-full"
                aria-label={m.settings_remote_access_listen_title()}
                disabled={locked}
                value={listen}
                onValueChange={(next) => {
                  if (next && next !== listen) {
                    chooseListen(next as HttpListen)
                  }
                }}
              >
                {LISTEN_SCOPES.map((scope) => (
                  <SegmentedButtonItem
                    key={scope}
                    value={scope}
                    disabled={scope === 'lan' && noLanNetwork}
                  >
                    {listenLabels[scope]}
                  </SegmentedButtonItem>
                ))}
              </SegmentedButton>
            </div>

            {listen !== 'local' && (
              <p
                role="note"
                data-slot="remote-access-network-warning"
                className="bg-error-container text-on-error-container rounded-medium px-4 py-3 text-sm"
              >
                {listen === 'lan'
                  ? m.settings_remote_access_listen_lan_warning()
                  : m.settings_remote_access_listen_any_warning()}
              </p>
            )}

            {listen === 'lan' && (
              <LanAllowList
                value={lanAllow}
                networks={networks.data}
                failed={networks.isError}
                disabled={locked}
                onChange={(lan_allow) => change({ lan_allow })}
              />
            )}
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <div className="flex flex-col gap-3" data-slot="remote-access-auth">
              <ItemLabel>
                <ItemLabelText>
                  {m.settings_remote_access_auth_title()}
                </ItemLabelText>

                <ItemLabelDescription>
                  {listen === 'local'
                    ? m.settings_remote_access_auth_description()
                    : m.settings_remote_access_auth_optional_unavailable()}
                </ItemLabelDescription>
              </ItemLabel>

              <SegmentedButton
                className="w-full"
                aria-label={m.settings_remote_access_auth_title()}
                disabled={locked}
                value={auth}
                onValueChange={(next) => {
                  if (next && next !== auth) {
                    change({ auth: next as HttpAuthentication })
                  }
                }}
              >
                {AUTH_MODES.map((mode) => (
                  <SegmentedButtonItem
                    key={mode}
                    value={mode}
                    disabled={mode === 'optional' && listen !== 'local'}
                  >
                    {authLabels[mode]}
                  </SegmentedButtonItem>
                ))}
              </SegmentedButton>
            </div>

            {auth === 'optional' && (
              <>
                <p
                  role="note"
                  data-slot="remote-access-auth-optional-warning"
                  className="bg-error-container text-on-error-container rounded-medium px-4 py-3 text-sm"
                >
                  {m.settings_remote_access_auth_optional_warning()}
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
                    limit={ANONYMOUS_PERMISSIONS_LIMIT}
                    disabled={locked}
                    onChange={(next) => change({ anonymous_permissions: next })}
                  />

                  <GrantReach held={anonymousPermissions} />
                </div>
              </>
            )}
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  )
}
