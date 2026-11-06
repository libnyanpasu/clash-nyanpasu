import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  cardVariants,
} from '@nyanpasu/ui/card'
import {
  Modal,
  ModalContent,
  ModalDescription,
  ModalTitle,
} from '@nyanpasu/ui/modal'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useIpLookupProviders, useSetting } from '@nyanpasu/query'
import type {
  IpLookupProvider,
  IpLookupProviderInfo,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import { stunChipVariants } from './stun-capabilities'

/** Service names are brands and stay untranslated. */
const PROVIDER_NAMES: Record<IpLookupProvider, string> = {
  dnspod: 'DNSPod',
  ip_sb: 'IP.SB',
  ipify: 'ipify',
  ident_me: 'ident.me',
}

function PrivacyNoticeDialog({
  provider,
  onAccept,
  onCancel,
}: {
  provider: IpLookupProviderInfo | null
  onAccept: (provider: IpLookupProvider) => void
  onCancel: () => void
}) {
  const name = provider ? PROVIDER_NAMES[provider.provider] : ''

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
    <section
      className="flex flex-col gap-3"
      data-slot="ip-lookup-provider-list"
    >
      <SettingsLabel>{m.settings_public_ip_lookup_label()}</SettingsLabel>

      <SettingsGroup data-slot="ip-lookup-provider-items">
        {providers?.map((provider) => {
          const name = PROVIDER_NAMES[provider.provider]

          return (
            <div
              key={provider.provider}
              className={cn(
                cardVariants(),
                'flex min-w-0 items-center gap-4 px-5 py-6',
              )}
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
            </div>
          )
        })}
      </SettingsGroup>

      <p className="text-on-surface-variant px-3 text-xs">
        {m.settings_public_ip_lookup_description()}
      </p>

      <PrivacyNoticeDialog
        provider={pending}
        onAccept={handleAccept}
        onCancel={() => setPending(null)}
      />
    </section>
  )
}
