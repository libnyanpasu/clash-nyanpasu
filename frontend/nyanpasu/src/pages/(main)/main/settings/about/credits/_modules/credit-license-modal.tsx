import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Modal, ModalClose, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { openExternal } from '../../../_modules/open-external'
import { creditPlatforms, creditSourceLabel } from './credit-labels'
import type { CreditPackage } from './use-open-source-credits'

/**
 * Shows a package's license texts. The page keeps the last selected package
 * while the dialog closes, so the exit animation still has its content.
 */
export default function CreditLicenseModal({
  pkg,
  texts,
  open,
  onOpenChange,
}: {
  pkg: CreditPackage | null
  texts: Record<string, string>
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const platforms = creditPlatforms(pkg?.platforms)

  return (
    <Modal open={open && pkg != null} onOpenChange={onOpenChange}>
      {pkg && (
        <ModalContent>
          <Card
            className="flex max-h-[90dvh] w-[min(48rem,calc(100vw-2rem))] flex-col"
            data-slot="credits-license-modal"
          >
            <CardHeader className="shrink-0 flex-col items-start gap-1">
              <ModalTitle className="break-all">{pkg.name}</ModalTitle>

              <p className="text-on-surface-variant text-sm">
                {[
                  pkg.version,
                  creditSourceLabel[pkg.source](),
                  pkg.license ?? m.settings_about_credits_no_license(),
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </p>
            </CardHeader>

            <CardContent asChild className="min-h-0 flex-1 overflow-hidden">
              <ScrollArea className="max-h-[60dvh]">
                <div className="flex flex-col gap-3">
                  {(pkg.notes || platforms) && (
                    <div
                      className="flex flex-col gap-1 text-sm"
                      data-slot="credits-license-modal-notes"
                    >
                      {pkg.notes && <p>{pkg.notes}</p>}

                      {platforms && (
                        <p className="text-on-surface-variant">
                          {m.settings_about_credits_platforms({ platforms })}
                        </p>
                      )}
                    </div>
                  )}

                  {pkg.textIds.map((id) => (
                    <pre
                      key={id}
                      className="bg-surface-variant/40 dark:bg-surface-variant/20 rounded-2xl p-4 font-mono text-xs break-words whitespace-pre-wrap"
                      data-slot="credits-license-text"
                    >
                      {texts[id]}
                    </pre>
                  ))}

                  {pkg.textIds.length === 0 && (
                    <p className="text-on-surface-variant text-sm">
                      {m.settings_about_credits_no_license_text()}
                    </p>
                  )}
                </div>
              </ScrollArea>
            </CardContent>

            <CardFooter className="shrink-0">
              <ModalClose>{m.common_close()}</ModalClose>

              {pkg.homepage && (
                <Button
                  variant="basic"
                  className="flex items-center gap-1"
                  onClick={() => openExternal(pkg.homepage!)}
                  data-slot="credits-license-modal-homepage"
                >
                  <OpenInNewRounded className="size-4" />
                  {m.settings_about_credits_homepage()}
                </Button>
              )}
            </CardFooter>
          </Card>
        </ModalContent>
      )}
    </Modal>
  )
}
