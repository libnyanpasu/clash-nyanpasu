import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { writeClipboardText } from '@/utils/clipboard'
import { useLocalNetworks, useRemoteAccess } from '@nyanpasu/query'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'

/**
 * The addresses the running server answers at: this computer's loopback, then every address of
 * this computer the listen scope reaches. Shown only while the server listens.
 */
export default function AccessAddresses() {
  const { status, http, openInBrowser } = useRemoteAccess()
  const [error, setError] = useState<string | null>(null)

  const listen = http?.listen ?? 'local'
  const lanAllow = http?.lan_allow ?? []
  const running = status.data?.enabled === true && status.data.port != null
  const networks = useLocalNetworks({
    enabled: running && listen !== 'local',
  })

  if (!running || status.data?.port == null) {
    return null
  }

  const scheme = status.data.tls ? 'https' : 'http'
  const port = status.data.port
  const reached =
    listen === 'local'
      ? []
      : (networks.data ?? []).filter(
          ({ network }) => listen === 'any' || lanAllow.includes(network),
        )
  const addresses = [
    {
      url: `${scheme}://127.0.0.1:${port}/`,
      detail: m.settings_remote_access_address_this_computer(),
    },
    ...reached.map(({ address, interface: name }) => ({
      url: `${scheme}://${address}:${port}/`,
      detail: name,
    })),
  ]

  const copy = (url: string) => {
    setError(null)
    writeClipboardText(url).catch((failure) => setError(formatError(failure)))
  }

  return (
    <SettingsCard data-slot="remote-access-addresses">
      <SettingsCardContent>
        <ItemLabel>
          <ItemLabelText>
            {m.settings_remote_access_addresses_title()}
          </ItemLabelText>

          <ItemLabelDescription>
            {listen === 'local'
              ? m.settings_remote_access_addresses_local_description()
              : m.settings_remote_access_addresses_network_description()}
          </ItemLabelDescription>
        </ItemLabel>

        <ul className="flex flex-col gap-1">
          {addresses.map(({ url, detail }) => (
            <li
              key={url}
              className="flex items-center gap-3"
              data-slot="remote-access-address"
            >
              <span className="flex min-w-0 flex-1 flex-col">
                <code className="text-sm break-all select-text">{url}</code>

                <span className="text-on-surface-variant text-xs">
                  {detail}
                </span>
              </span>

              <Button
                icon
                aria-label={`${m.common_copy()} ${url}`}
                onClick={() => copy(url)}
              >
                <ContentCopyRounded />
              </Button>
            </li>
          ))}
        </ul>

        <div>
          <Button
            variant="flat"
            data-slot="remote-access-open"
            disabled={openInBrowser.isPending}
            onClick={() => {
              setError(null)
              openInBrowser.mutate(undefined, {
                onError: (failure) => setError(formatError(failure)),
              })
            }}
          >
            {m.settings_remote_access_open()}
          </Button>
        </div>

        {error && (
          <p role="alert" className="text-error text-sm">
            {error}
          </p>
        )}
      </SettingsCardContent>
    </SettingsCard>
  )
}
