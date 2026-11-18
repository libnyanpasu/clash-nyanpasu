import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { formatDate } from '@/utils/date'
import { ask } from '@/utils/notification'
import { useRemoteAccess } from '@nyanpasu/query'
import type { TlsCertificateSummary } from '@nyanpasu/rpc/types'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../../../_modules/settings-card'

/**
 * The fingerprint of the certificate the HTTPS listener presents, which a browser warning or
 * `nyanpasu-cli` is compared with, and the replacement of that certificate.
 *
 * The replacement stays available while the listener presents nothing (`tls` is undefined),
 * because a certificate that cannot be served is repaired by replacing it.
 */
export default function CertificateSettings({
  tls,
  bindOutsideCertificate,
}: {
  tls?: TlsCertificateSummary
  bindOutsideCertificate: boolean
}) {
  const { resetCertificate } = useRemoteAccess()
  const [error, setError] = useState<string | null>(null)

  const handleReset = async () => {
    setError(null)

    const confirmed = await ask(
      m.settings_remote_access_certificate_reset_confirm(),
      { kind: 'warning' },
    )
    if (!confirmed) {
      return
    }

    try {
      await resetCertificate.mutateAsync()
    } catch (failure) {
      setError(formatError(failure))
    }
  }

  return (
    <div className="flex flex-col gap-2" data-slot="remote-access-certificate">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_remote_access_certificate_title()}
        </ItemLabelText>

        <ItemLabelDescription>
          {m.settings_remote_access_certificate_description()}
        </ItemLabelDescription>
      </ItemLabel>

      {tls ? (
        <>
          <code
            className="text-sm break-all select-text"
            data-slot="remote-access-fingerprint"
          >
            {tls.fingerprint}
          </code>

          <ItemLabelDescription>
            {m.settings_remote_access_certificate_expires({
              date: formatDate(tls.not_after_ms),
            })}
          </ItemLabelDescription>
        </>
      ) : (
        <ItemLabelDescription data-slot="remote-access-certificate-none">
          {m.settings_remote_access_certificate_none()}
        </ItemLabelDescription>
      )}

      {bindOutsideCertificate && (
        <p
          role="note"
          data-slot="remote-access-certificate-address"
          className="bg-error-container text-on-error-container rounded-xl px-4 py-3 text-sm"
        >
          {m.settings_remote_access_certificate_address_mismatch()}
        </p>
      )}

      <div>
        <Button
          variant="stroked"
          disabled={resetCertificate.isPending}
          onClick={handleReset}
        >
          {m.settings_remote_access_certificate_reset()}
        </Button>
      </div>

      {error && (
        <p role="alert" className="text-error text-sm">
          {error}
        </p>
      )}
    </div>
  )
}
