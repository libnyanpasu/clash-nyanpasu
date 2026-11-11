import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalDescription,
  ModalTitle,
  ModalTrigger,
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
}: {
  provider: IpLookupProviderInfo
  onAccept: (provider: IpLookupProvider) => void
}) {
  const name = IP_LOOKUP_PROVIDER_NAMES[provider.provider]

  return (
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
          <ModalClose>{m.common_cancel()}</ModalClose>

          <ModalClose asChild>
            <Button
              variant="flat"
              onClick={() => provider && onAccept(provider.provider)}
            >
              {m.settings_public_ip_lookup_notice_accept()}
            </Button>
          </ModalClose>
        </CardFooter>
      </Card>
    </ModalContent>
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

  const save = useLockFn(async (next: IpLookupProvider[]) => {
    try {
      await upsert(next)
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  })

  const handleAccept = async (provider: IpLookupProvider) => {
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
            <Modal key={provider.provider}>
              <SettingsListItem
                data-slot="ip-lookup-provider-item"
                data-provider={provider.provider}
              >
                <div className="min-w-0 flex-1">
                  <p className="text-base font-medium">{name}</p>

                  <span className={cn(stunChipVariants(), 'mt-1')}>
                    {new URL(provider.ipv4_url).host}
                  </span>
                </div>

                <ModalTrigger
                  asChild
                  onClick={(event) => {
                    if (accepted?.includes(provider.provider)) {
                      event.preventDefault()
                      save(
                        accepted.filter((item) => item !== provider.provider),
                      )
                    }
                  }}
                >
                  <Switch
                    aria-label={name}
                    checked={accepted?.includes(provider.provider) ?? false}
                    disabled={accepted === undefined || isPending}
                  />
                </ModalTrigger>
              </SettingsListItem>
              <PrivacyNoticeDialog
                provider={provider}
                onAccept={handleAccept}
              />
            </Modal>
          )
        })}
      </SettingsListContent>
    </SettingsList>
  )
}
