import { filesize } from 'filesize'
import { useEffect, useState } from 'react'
import Markdown from 'react-markdown'
import { ActionSwapText } from '@nyanpasu/ui/action-swap-text'
import { Button, type ButtonProps } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { CircularProgress, LinearProgress } from '@nyanpasu/ui/progress'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { commands } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { isTauri } from '@tauri-apps/api/core'
import {
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card'

const GITHUB_RELEASES_URL =
  'https://github.com/libnyanpasu/clash-nyanpasu/releases'

async function openExternal(url: string) {
  try {
    if (isTauri()) await commands.openThat(url)
    else window.open(url, '_blank', 'noopener,noreferrer')
  } catch (error) {
    message(formatError(error), { kind: 'error', error })
  }
}

function openReleases() {
  return openExternal(GITHUB_RELEASES_URL)
}

function sourceLabel(source: string | null | undefined) {
  if (source === 'nyanpasu') return m.update_source_nyanpasu()
  if (source === 'github') return m.update_source_github()
  if (source === 'ghfast') return m.update_source_ghfast()
  return null
}

type UpdateActionButtonProps = Omit<ButtonProps, 'children' | 'loading'> & {
  status: string
  lastCheckedLabel: string | null
  loading?: boolean
}

function UpdateActionButton({
  status,
  lastCheckedLabel,
  loading = false,
  disabled,
  ...buttonProps
}: UpdateActionButtonProps) {
  const showLastChecked = Boolean(lastCheckedLabel) && !disabled
  const button = (
    <Button {...buttonProps} disabled={disabled}>
      <div className="flex w-full min-w-0 items-center justify-center gap-1.5 text-center whitespace-nowrap">
        {loading && (
          <CircularProgress
            className="size-4 shrink-0"
            indeterminate
            aria-hidden
          />
        )}
        <ActionSwapText
          className="max-w-full leading-tight font-medium whitespace-nowrap"
          aria-live="polite"
          aria-atomic="true"
          value={status}
        />
      </div>
    </Button>
  )

  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      {showLastChecked && lastCheckedLabel && (
        <TooltipContent>
          <span>{lastCheckedLabel}</span>
        </TooltipContent>
      )}
    </Tooltip>
  )
}

export default function UpdateControls({
  openChangelog = false,
}: {
  openChangelog?: boolean
}) {
  const {
    snapshot,
    isDesktop,
    isLoading,
    isPending,
    check,
    download,
    cancelDownload,
    install,
    discardPackage,
  } = useNyanpasuUpdate()
  const [changelogOpen, setChangelogOpen] = useState(false)
  const phase = snapshot?.phase ?? 'idle'
  const release = snapshot?.release
  const lastCheckedLabel = snapshot?.last_checked_at
    ? m.settings_about_update_last_checked({
        date: new Date(snapshot.last_checked_at).toLocaleString(),
      })
    : null

  useEffect(() => {
    if (openChangelog && release?.body) setChangelogOpen(true)
  }, [openChangelog, release?.body])
  const progress =
    snapshot?.total && snapshot.total > 0
      ? Math.min((snapshot.downloaded / snapshot.total) * 100, 100)
      : undefined
  const run = useLockFn(async (operation: () => Promise<unknown>) => {
    try {
      await operation()
    } catch (error) {
      message(formatError(error), {
        kind: 'error',
        error,
        title: m.settings_about_update_error_title(),
      })
    }
  })

  const checkUpdate = () => run(check)
  const downloadUpdate = () => run(download)
  const cancelUpdateDownload = () => run(cancelDownload)
  const installUpdate = () => run(install)
  const discardUpdatePackage = () => run(discardPackage)

  const phaseLabel: Record<string, string> = {
    idle: m.settings_about_update_idle(),
    checking: m.settings_about_update_checking(),
    up_to_date: m.settings_label_about_update_no_update(),
    available: m.settings_label_about_update_has_new_version(),
    downloading: m.settings_about_update_downloading(),
    cancelling: m.settings_about_update_cancelling(),
    cancelled: m.settings_about_update_cancelled(),
    verifying: m.settings_about_update_verifying(),
    ready: m.settings_about_update_ready(),
    installing: m.settings_label_about_update_installing(),
    failed: m.settings_about_update_failed(),
  }

  const statusLabel = phaseLabel[phase] ?? phaseLabel.failed

  const action = (() => {
    if (!isDesktop) {
      return (
        <Button className="w-full" variant="flat" onClick={openReleases}>
          {m.settings_label_about_update_to_github_releases()}
        </Button>
      )
    }
    if (!snapshot) {
      return (
        <UpdateActionButton
          variant="flat"
          className="w-full"
          disabled={isLoading || isPending}
          onClick={checkUpdate}
          status={isLoading ? m.settings_about_update_loading() : statusLabel}
          lastCheckedLabel={lastCheckedLabel}
          loading={isLoading || isPending}
        />
      )
    }
    if (!snapshot.supported) {
      return (
        <Button className="w-full" variant="flat" onClick={openReleases}>
          {m.settings_label_about_update_to_github_releases()}
        </Button>
      )
    }
    if (phase === 'checking') {
      return (
        <UpdateActionButton
          className="h-auto min-h-10 w-full py-2"
          variant="flat"
          disabled
          onClick={checkUpdate}
          status={statusLabel}
          lastCheckedLabel={lastCheckedLabel}
          loading
        />
      )
    }
    if (
      phase === 'cancelling' ||
      phase === 'verifying' ||
      phase === 'installing'
    ) {
      return (
        <UpdateActionButton
          className="h-auto min-h-10 w-full py-2"
          variant="flat"
          disabled
          status={statusLabel}
          lastCheckedLabel={lastCheckedLabel}
          loading={phase === 'cancelling'}
        />
      )
    }
    if (phase === 'downloading') {
      return (
        <UpdateActionButton
          variant="flat"
          className="h-auto min-h-10 w-full py-2"
          disabled={isPending}
          onClick={cancelUpdateDownload}
          status={statusLabel}
          lastCheckedLabel={lastCheckedLabel}
          loading={isPending}
        />
      )
    }
    if (phase === 'ready') {
      return (
        <div className="flex w-full flex-col gap-2">
          <UpdateActionButton
            className="h-auto min-h-10 w-full py-2"
            variant="flat"
            disabled={isPending}
            onClick={installUpdate}
            status={statusLabel}
            lastCheckedLabel={lastCheckedLabel}
          />
          <Button
            variant="stroked"
            className="w-full"
            disabled={isPending}
            onClick={discardUpdatePackage}
          >
            {m.settings_about_update_discard()}
          </Button>
        </div>
      )
    }
    if (
      phase === 'available' ||
      phase === 'cancelled' ||
      (phase === 'failed' && release)
    ) {
      return (
        <UpdateActionButton
          className="h-auto min-h-10 w-full py-2"
          variant="flat"
          disabled={isPending}
          onClick={downloadUpdate}
          status={statusLabel}
          lastCheckedLabel={lastCheckedLabel}
        />
      )
    }
    return (
      <UpdateActionButton
        variant="flat"
        className="h-auto min-h-10 w-full py-2"
        disabled={isPending || isLoading}
        onClick={checkUpdate}
        status={statusLabel}
        lastCheckedLabel={lastCheckedLabel}
        loading={isPending || isLoading}
      />
    )
  })()

  return (
    <>
      <SettingsCardContent
        className="gap-4 pt-0 pb-2"
        data-slot="about-update-controls"
        data-phase={phase}
      >
        <div className="flex w-full flex-col items-center gap-4">
          <div className="w-full min-w-0 text-center">
            {release && (
              <p className="text-on-surface-variant mt-1 text-sm">
                {m.settings_about_update_version({ version: release.version })}
                {release.date
                  ? ` · ${new Date(release.date).toLocaleDateString()}`
                  : ''}
              </p>
            )}
          </div>
          {release?.body && (
            <Modal open={changelogOpen} onOpenChange={setChangelogOpen}>
              <ModalTrigger asChild>
                <Button className="w-full" variant="flat">
                  {m.settings_about_update_changelog()}
                </Button>
              </ModalTrigger>
              <ModalContent>
                <Card className="flex max-h-[90dvh] max-w-3xl min-w-80 flex-col">
                  <CardHeader className="shrink-0">
                    <ModalTitle>
                      {m.settings_about_update_changelog_title({
                        version: release.version,
                      })}
                    </ModalTitle>
                  </CardHeader>
                  <CardContent
                    asChild
                    className="min-h-0 flex-1 overflow-hidden"
                  >
                    <ScrollArea className="max-h-[65dvh]">
                      <Markdown
                        components={{
                          a(props) {
                            const { children, node, ...rest } = props
                            return (
                              <a
                                {...rest}
                                onClick={(event) => {
                                  event.preventDefault()
                                  event.stopPropagation()
                                  if (typeof node?.properties.href === 'string')
                                    openExternal(node.properties.href)
                                }}
                              >
                                {children}
                              </a>
                            )
                          },
                        }}
                      >
                        {release.body}
                      </Markdown>
                    </ScrollArea>
                  </CardContent>
                  <CardFooter className="shrink-0 justify-end">
                    <ModalClose>{m.common_close()}</ModalClose>
                  </CardFooter>
                </Card>
              </ModalContent>
            </Modal>
          )}
        </div>

        {(phase === 'downloading' || phase === 'cancelling') && (
          <div className="flex flex-col gap-2" aria-live="polite">
            <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
              <span>
                {filesize(snapshot?.downloaded ?? 0, { standard: 'iec' })}
                {snapshot?.total != null
                  ? ` / ${filesize(snapshot.total, { standard: 'iec' })}`
                  : ` · ${m.settings_about_update_total_unknown()}`}
              </span>
              <span>
                {filesize(snapshot?.speed ?? 0, { standard: 'iec' })}/s
              </span>
            </div>
            <LinearProgress
              className="w-full"
              value={progress}
              indeterminate={progress === undefined}
            />
            {sourceLabel(snapshot?.source) && (
              <p className="text-on-surface-variant text-xs">
                {m.settings_about_update_current_source({
                  source: sourceLabel(snapshot?.source) ?? '',
                })}
              </p>
            )}
          </div>
        )}
        {phase === 'verifying' && (
          <LinearProgress className="w-full" indeterminate />
        )}
        {phase === 'installing' && (
          <p className="text-on-surface-variant text-sm" aria-live="polite">
            {m.settings_about_update_installing_description()}
          </p>
        )}
        {snapshot?.error && (
          <p className="text-error text-sm" role="alert">
            {snapshot.error}
          </p>
        )}
        {snapshot?.supported === false && (
          <p className="text-on-surface-variant text-sm">
            {m.settings_about_update_unsupported()}
          </p>
        )}
      </SettingsCardContent>
      {action && (
        <SettingsCardFooter className="flex-col gap-2 pt-0">
          <div className="flex w-full flex-col gap-2">
            {(phase === 'available' || phase === 'ready') && (
              <Button
                variant="stroked"
                className="w-full"
                disabled={isPending}
                onClick={checkUpdate}
              >
                {m.settings_about_update_check_again()}
              </Button>
            )}
            {action}
          </div>
        </SettingsCardFooter>
      )}
    </>
  )
}
