import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalContent,
  ModalDescription,
  ModalTitle,
} from '@nyanpasu/ui/modal'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { IP_LOOKUP_PROVIDER_NAMES } from '@/utils/ip-lookup-provider'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useIpLookupProviders, useSetting } from '@nyanpasu/query'
import type {
  IpLookupProvider,
  IpLookupProviderInfo,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { SettingsLabel } from '../../../../_modules/settings-card'
import {
  SettingsList,
  SettingsListContent,
  SettingsListItem,
} from '../../../../_modules/settings-list'
import { stunChipVariants } from './stun-capabilities'

function PrivacyNoticeDialog({
  provider,
  onAccept,
  onCancel,
}: {
  provider: IpLookupProviderInfo | null
  onAccept: (provider: IpLookupProvider) => void
  onCancel: () => void
}) {
  const name = provider ? IP_LOOKUP_PROVIDER_NAMES[provider.provider] : ''

  return (
    <Modal open={provider != null} onOpenChange={(next) => !next && onCancel()}>
      <ModalContent>
        <Card
          className="w-[28rem] max-w-full"
          data-slot="ip-lookup-privacy-notice"
        >
          <CardHeader>
            <ModalTitle>
              {m.settings_public_ip_lookup_notice_title({ provider: name })}
            </ModalTitle>
          </CardHeader>

          <CardContent className="flex flex-col gap-3 text-sm">
            <ModalDescription className="text-on-surface-variant">
              {m.settings_public_ip_lookup_notice_description({
                provider: name,
              })}
            </ModalDescription>

            {provider && (
              <div data-slot="ip-lookup-privacy-notice-endpoints">
                <p className="text-on-surface-variant">
                  {m.settings_public_ip_lookup_notice_endpoints()}
                </p>

                <ul className="mt-1 font-mono text-xs break-all">
                  <li>{provider.ipv4_url}</li>
                  <li>{provider.ipv6_url}</li>
                </ul>
              </div>
            )}
          </CardContent>

          <CardFooter className="gap-1">
            <Button onClick={onCancel}>{m.common_cancel()}</Button>

            <Button
              variant="flat"
              onClick={() => provider && onAccept(provider.provider)}
            >
              {m.settings_public_ip_lookup_notice_accept()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

/**
 * HTTP services asked only when no STUN server answers. Each one is off until
 * the user accepts its privacy notice.
 */
export default function IpLookupProviderList() {
  const {
    value: accepted,
    upsert,
    isPending,
  } = useSetting('ip_lookup_providers')
  const { data: providers } = useIpLookupProviders()

  const [pending, setPending] = useState<IpLookupProviderInfo | null>(null)

  const save = useLockFn(async (next: IpLookupProvider[]) => {
    try {
      await upsert(next)
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  })

  const handleToggle = (provider: IpLookupProviderInfo, checked: boolean) => {
    if (!accepted) return

    if (checked) {
      setPending(provider)
    } else {
      save(accepted.filter((item) => item !== provider.provider))
    }
  }

  const handleAccept = async (provider: IpLookupProvider) => {
    setPending(null)
    if (accepted && !accepted.includes(provider)) {
      await save([...accepted, provider])
    }
  }

  return (
    <SettingsList data-slot="ip-lookup-provider-list">
      <SettingsLabel>{m.settings_public_ip_lookup_label()}</SettingsLabel>

      <SettingsListContent
        data-slot="ip-lookup-provider-items"
        empty={providers?.length === 0}
        description={<p>{m.settings_public_ip_lookup_description()}</p>}
      >
        {providers?.map((provider) => {
          const name = IP_LOOKUP_PROVIDER_NAMES[provider.provider]

          return (
            <SettingsListItem
              key={provider.provider}
              data-slot="ip-lookup-provider-item"
              data-provider={provider.provider}
            >
              <div className="min-w-0 flex-1">
                <p className="text-base font-medium">{name}</p>

                <span className={cn(stunChipVariants(), 'mt-1')}>
                  {new URL(provider.ipv4_url).host}
                </span>
              </div>

              <Switch
                aria-label={name}
                checked={accepted?.includes(provider.provider) ?? false}
                disabled={accepted === undefined || isPending}
                onCheckedChange={(checked) => handleToggle(provider, checked)}
              />
            </SettingsListItem>
          )
        })}
      </SettingsListContent>

      <PrivacyNoticeDialog
        provider={pending}
        onAccept={handleAccept}
        onCancel={() => setPending(null)}
      />
    </SettingsList>
  )
}
