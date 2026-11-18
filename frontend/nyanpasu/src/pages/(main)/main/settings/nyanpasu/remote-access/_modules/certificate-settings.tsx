import { useState, type ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { formatDate } from '@/utils/date'
import { ask } from '@/utils/notification'
import { useRemoteAccess, useRemoteAccessCertificate } from '@nyanpasu/query'
import type { TlsCertificateDetails } from '@nyanpasu/rpc/types'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../../../_modules/settings-card'

/** One labelled field of the certificate. */
function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col" data-slot="remote-access-certificate-field">
      <dt className="text-on-surface-variant text-xs">{label}</dt>

      <dd className="text-sm break-all select-text">{children}</dd>
    </div>
  )
}

/** The issuer, marked when the certificate signs itself. */
function issuerOf(certificate: TlsCertificateDetails) {
  return certificate.issuer === certificate.subject
    ? `${certificate.issuer} (${m.settings_remote_access_certificate_self_signed()})`
    : certificate.issuer
}

/** Every field a certificate viewer shows, and the platform's own viewer for it. */
function CertificateDetailsModal({
  certificate,
}: {
  certificate: TlsCertificateDetails
}) {
  const { openInViewer } = useRemoteAccessCertificate()
  const [error, setError] = useState<string | null>(null)

  const list = (values: string[]) =>
    values.length > 0 ? values.join(', ') : '—'

  return (
    <ModalContent onOpenAutoFocus={() => setError(null)}>
      <Card
        className="w-[32rem] max-w-full"
        data-slot="remote-access-certificate-dialog"
      >
        <CardHeader>
          <ModalTitle>
            {m.settings_remote_access_certificate_details()}
          </ModalTitle>
        </CardHeader>

        <CardContent>
          <dl className="flex flex-col gap-3">
            <Field label={m.settings_remote_access_certificate_subject()}>
              {certificate.subject}
            </Field>

            <Field label={m.settings_remote_access_certificate_issuer()}>
              {issuerOf(certificate)}
            </Field>

            <Field label={m.settings_remote_access_certificate_serial()}>
              <code>{certificate.serial}</code>
            </Field>

            <Field label={m.settings_remote_access_certificate_not_before()}>
              {formatDate(certificate.not_before_ms)}
            </Field>

            <Field label={m.settings_remote_access_certificate_not_after()}>
              {formatDate(certificate.not_after_ms)}
            </Field>

            <Field label={m.settings_remote_access_certificate_dns_names()}>
              {list(certificate.dns_names)}
            </Field>

            <Field label={m.settings_remote_access_certificate_ip_addresses()}>
              {list(certificate.ip_addresses)}
            </Field>

            <Field
              label={m.settings_remote_access_certificate_signature_algorithm()}
            >
              {certificate.signature_algorithm}
            </Field>

            <Field
              label={m.settings_remote_access_certificate_public_key_algorithm()}
            >
              {certificate.public_key_algorithm}
            </Field>

            <Field label={m.settings_remote_access_certificate_sha256()}>
              <code>{certificate.sha256}</code>
            </Field>

            <Field label={m.settings_remote_access_certificate_sha1()}>
              <code>{certificate.sha1}</code>
            </Field>
          </dl>

          {error && (
            <p role="alert" className="text-error mt-3 text-sm">
              {error}
            </p>
          )}
        </CardContent>

        <CardFooter className="gap-1">
          <Button
            variant="stroked"
            disabled={openInViewer.isPending}
            onClick={() => {
              setError(null)
              openInViewer.mutate(undefined, {
                onError: (failure) => setError(formatError(failure)),
              })
            }}
          >
            {m.settings_remote_access_certificate_open_viewer()}
          </Button>

          <ModalClose>{m.common_close()}</ModalClose>
        </CardFooter>
      </Card>
    </ModalContent>
  )
}

/**
 * The saved certificate the HTTPS listener presents: its fingerprint, issuer and validity, its
 * details and the platform's viewer, and its replacement. It is read from the saved settings, so
 * it shows while nothing listens; the replacement stays available when none was generated yet or
 * the listener cannot serve the saved one.
 */
export default function CertificateSettings() {
  const { resetCertificate } = useRemoteAccess()
  const { query } = useRemoteAccessCertificate()
  const [error, setError] = useState<string | null>(null)

  const certificate = query.data ?? null

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

      {certificate ? (
        <dl className="flex flex-col gap-2">
          <Field label={m.settings_remote_access_certificate_sha256()}>
            <code data-slot="remote-access-fingerprint">
              {certificate.sha256}
            </code>
          </Field>

          <Field label={m.settings_remote_access_certificate_issuer()}>
            {issuerOf(certificate)}
          </Field>

          <Field label={m.settings_remote_access_certificate_validity()}>
            {m.settings_remote_access_certificate_validity_range({
              from: formatDate(certificate.not_before_ms),
              to: formatDate(certificate.not_after_ms),
            })}
          </Field>
        </dl>
      ) : (
        query.isSuccess && (
          <ItemLabelDescription data-slot="remote-access-certificate-none">
            {m.settings_remote_access_certificate_none()}
          </ItemLabelDescription>
        )
      )}

      <div className="flex flex-wrap gap-2">
        {certificate && (
          <Modal>
            <ModalTrigger asChild>
              <Button variant="stroked">
                {m.settings_remote_access_certificate_details()}
              </Button>
            </ModalTrigger>

            <CertificateDetailsModal certificate={certificate} />
          </Modal>
        )}

        <Button
          variant="stroked"
          disabled={resetCertificate.isPending}
          onClick={handleReset}
        >
          {m.settings_remote_access_certificate_reset()}
        </Button>
      </div>

      {(error || query.isError) && (
        <p role="alert" className="text-error text-sm">
          {error ?? m.settings_remote_access_certificate_error()}
        </p>
      )}
    </div>
  )
}
