import CheckCircleRounded from '~icons/material-symbols/check-circle-rounded'
import DeployedCodeUpdateOutlineRounded from '~icons/material-symbols/deployed-code-update-outline-rounded'
import DescriptionOutlineRounded from '~icons/material-symbols/description-outline-rounded'
import DownloadRounded from '~icons/material-symbols/download-rounded'
import ErrorRounded from '~icons/material-symbols/error-rounded'
import MoreHorizRounded from '~icons/material-symbols/more-horiz-rounded'
import SyncRounded from '~icons/material-symbols/sync-rounded'
import UpdateRounded from '~icons/material-symbols/update-rounded'
import { cva } from 'class-variance-authority'
import { filesize } from 'filesize'
import {
  AnimatePresence,
  motion,
  useReducedMotion,
  type Transition,
} from 'motion/react'
import type { ComponentProps, ComponentType } from 'react'
import { Button, type ButtonProps } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
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
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import type { AppUpdatePhase } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { AppUpdateChangelog } from '../../../_modules/app-update-changelog'
import { openExternal } from '../../../_modules/open-external'
import {
  SettingsCardContent,
  SettingsCardFooter,
} from '../../../../_modules/settings-card'

const GITHUB_RELEASES_URL =
  'https://github.com/libnyanpasu/clash-nyanpasu/releases'

function openReleases() {
  return openExternal(GITHUB_RELEASES_URL)
}

function sourceLabel(source: string | null | undefined) {
  if (source === 'nyanpasu') return m.update_source_nyanpasu()
  if (source === 'github') return m.update_source_github()
  if (source === 'ghfast') return m.update_source_ghfast()
  return null
}

// Springs carry their velocity into the next target, so sections that open and
// close while the phase keeps changing do not restart from a standstill the way
// a fixed tween does. Opacity stays a tween so content never overshoots.
const expandTransition = {
  default: { type: 'spring', duration: 0.4, bounce: 0 },
  opacity: { duration: 0.2, ease: [0.2, 0, 0, 1] },
} satisfies Transition

const iconSwapTransition = {
  type: 'spring',
  duration: 0.35,
  bounce: 0.3,
} satisfies Transition

const instantTransition = { duration: 0 } satisfies Transition

const labelTransition = { duration: 0.12, ease: 'easeOut' } satisfies Transition

const updateStatusVariants = cva(
  'flex flex-col rounded-3xl p-4 transition-colors duration-300',
  {
    variants: {
      tone: {
        neutral:
          'bg-surface-variant/30 text-on-surface dark:bg-surface-variant/10',
        primary: 'bg-primary-container text-on-primary-container',
        error: 'bg-error-container text-on-error-container',
      },
    },
    defaultVariants: { tone: 'neutral' },
  },
)

const updateStatusIconVariants = cva(
  'relative grid size-10 shrink-0 place-content-center rounded-full transition-colors duration-300',
  {
    variants: {
      tone: {
        neutral: 'bg-primary-container text-on-primary-container',
        primary: 'bg-primary text-on-primary',
        error: 'bg-error text-on-error',
      },
    },
    defaultVariants: { tone: 'neutral' },
  },
)

const STATUS_ICONS: Partial<
  Record<AppUpdatePhase, ComponentType<{ className?: string }>>
> = {
  up_to_date: CheckCircleRounded,
  available: DownloadRounded,
  downloading: DownloadRounded,
  ready: DeployedCodeUpdateOutlineRounded,
  failed: ErrorRounded,
  checking: SyncRounded,
  cancelling: SyncRounded,
  verifying: SyncRounded,
  installing: SyncRounded,
}

const SPINNING_PHASES = ['checking', 'cancelling', 'verifying', 'installing']

function UpdateLabel({
  value,
  ...props
}: Omit<ComponentProps<'div'>, 'children'> & { value: string }) {
  const reducedMotion = useReducedMotion()

  return (
    <div {...props} data-slot="about-update-label">
      <motion.span
        key={value}
        className="block"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={reducedMotion ? instantTransition : labelTransition}
      >
        {value}
      </motion.span>
    </div>
  )
}

type UpdateActionButtonProps = Omit<ButtonProps, 'children' | 'loading'> & {
  label: string
  lastCheckedLabel: string | null
  loading?: boolean
}

function UpdateActionButton({
  label,
  lastCheckedLabel,
  loading = false,
  disabled,
  ...buttonProps
}: UpdateActionButtonProps) {
  const showLastChecked = Boolean(lastCheckedLabel) && !disabled
  const button = (
    <Button {...buttonProps} disabled={disabled}>
      <div className="grid w-full min-w-0 grid-cols-[1rem_minmax(0,1fr)_1rem] items-center gap-1.5 text-center whitespace-nowrap">
        <span className="size-4" aria-hidden>
          {loading && <CircularProgress className="size-4" indeterminate />}
        </span>
        <UpdateLabel
          className="min-w-0 leading-tight font-medium whitespace-nowrap"
          aria-live="polite"
          aria-atomic="true"
          value={label}
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

export default function UpdateControls() {
  const reducedMotion = useReducedMotion()
  const {
    currentVersion,
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
  const transition = reducedMotion ? instantTransition : expandTransition
  const iconTransition = reducedMotion ? instantTransition : iconSwapTransition
  const phase = snapshot?.phase ?? 'idle'
  const release = snapshot?.release
  const lastCheckedLabel = snapshot?.last_checked_at
    ? m.settings_about_update_last_checked({
        date: new Date(snapshot.last_checked_at).toLocaleString(),
      })
    : null

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

  const phaseLabel: Record<AppUpdatePhase, string> = {
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

  const statusLabel = phaseLabel[phase]

  const tone = (() => {
    if (phase === 'failed') return 'error'
    if (phase === 'available' || phase === 'ready') return 'primary'
    return 'neutral'
  })()
  const StatusIcon = STATUS_ICONS[phase] ?? UpdateRounded
  const showStatus = Boolean(snapshot?.supported) && phase !== 'idle'
  const statusDetail = release
    ? [
        m.settings_about_update_version({ version: release.version }),
        release.date ? new Date(release.date).toLocaleDateString() : null,
      ]
        .filter(Boolean)
        .join(' · ')
    : null

  const canUpdate = isDesktop && snapshot?.supported !== false
  const isTransitioning = SPINNING_PHASES.includes(phase)
  const showMore =
    canUpdate &&
    Boolean(release) &&
    ['available', 'cancelled', 'failed', 'ready'].includes(phase)
  const action = (() => {
    if (!canUpdate) {
      return {
        label: m.settings_label_about_update_to_github_releases(),
        handleClick: openReleases,
      }
    }
    if (!snapshot && isLoading) {
      return {
        label: m.settings_about_update_loading(),
        handleClick: checkUpdate,
      }
    }
    if (phase === 'downloading' || phase === 'cancelling') {
      return {
        label: m.settings_about_update_cancel_download(),
        handleClick: cancelUpdateDownload,
      }
    }
    if (phase === 'ready' || phase === 'verifying' || phase === 'installing') {
      return {
        label: m.settings_about_update_install(),
        handleClick: installUpdate,
      }
    }
    if (
      release &&
      (phase === 'available' || phase === 'cancelled' || phase === 'failed')
    ) {
      return {
        label:
          phase === 'failed'
            ? m.settings_about_update_retry_download()
            : m.settings_label_about_update_to_update_button(),
        handleClick: downloadUpdate,
      }
    }
    return { label: m.settings_about_update_idle(), handleClick: checkUpdate }
  })()

  return (
    <>
      <SettingsCardContent
        className="gap-3 pt-0 pb-2"
        data-slot="about-update-controls"
        data-phase={phase}
      >
        <AnimatePresence initial={false}>
          {isDesktop && showStatus && (
            <motion.div
              key="status"
              className="overflow-hidden"
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={transition}
            >
              <div
                className={updateStatusVariants({ tone })}
                data-slot="about-update-status"
              >
                <div className="flex items-center gap-3">
                  <div
                    className={updateStatusIconVariants({ tone })}
                    data-slot="about-update-status-icon"
                  >
                    <AnimatePresence mode="popLayout" initial={false}>
                      <motion.span
                        key={isTransitioning ? 'spinning' : phase}
                        className="grid place-content-center"
                        initial={{ scale: 0.4, rotate: -90, opacity: 0 }}
                        animate={{ scale: 1, rotate: 0, opacity: 1 }}
                        exit={{ scale: 0.4, rotate: 90, opacity: 0 }}
                        transition={iconTransition}
                      >
                        <StatusIcon
                          className={cn(
                            'size-6',
                            isTransitioning && 'animate-spin',
                          )}
                        />
                      </motion.span>
                    </AnimatePresence>
                  </div>

                  <div
                    className="flex min-w-0 flex-1 flex-col gap-0.5"
                    data-slot="about-update-status-content"
                  >
                    <UpdateLabel
                      className="text-sm font-medium"
                      role="status"
                      aria-atomic="true"
                      value={statusLabel}
                    />

                    {statusDetail && (
                      <span className="text-xs opacity-80">{statusDetail}</span>
                    )}

                    {snapshot?.error && (
                      <span className="text-sm break-words" role="alert">
                        {snapshot.error}
                      </span>
                    )}
                  </div>
                  {release && (
                    <Modal>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <ModalTrigger asChild>
                            <Button
                              icon
                              className="size-10 shrink-0"
                              variant="basic"
                              aria-label={m.settings_about_update_changelog()}
                            >
                              <DescriptionOutlineRounded className="size-5" />
                            </Button>
                          </ModalTrigger>
                        </TooltipTrigger>
                        <TooltipContent>
                          {m.settings_about_update_changelog()}
                        </TooltipContent>
                      </Tooltip>
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
                              <AppUpdateChangelog
                                from={currentVersion}
                                to={release.version}
                                fallbackBody={release.body}
                              />
                            </ScrollArea>
                          </CardContent>
                          <CardFooter className="shrink-0 justify-end">
                            <ModalClose>{m.common_close()}</ModalClose>
                          </CardFooter>
                        </Card>
                      </ModalContent>
                    </Modal>
                  )}
                  {showMore && (
                    <motion.div
                      key="more"
                      className="shrink-0"
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      transition={
                        reducedMotion ? instantTransition : labelTransition
                      }
                    >
                      <DropdownMenu align="end">
                        <DropdownMenuTrigger asChild>
                          <Button
                            icon
                            className="size-10 shrink-0"
                            disabled={isPending}
                            aria-label={m.settings_about_update_more_actions()}
                          >
                            <MoreHorizRounded className="size-5" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent>
                          <DropdownMenuItem
                            disabled={isPending}
                            onSelect={checkUpdate}
                          >
                            {m.settings_about_update_check_again()}
                          </DropdownMenuItem>
                          {phase === 'ready' && (
                            <DropdownMenuItem
                              disabled={isPending}
                              onSelect={discardUpdatePackage}
                            >
                              {m.settings_about_update_discard()}
                            </DropdownMenuItem>
                          )}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </motion.div>
                  )}
                </div>
                <AnimatePresence initial={false}>
                  {(phase === 'downloading' || phase === 'cancelling') && (
                    <motion.div
                      key="download"
                      className="overflow-hidden"
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={transition}
                    >
                      <div
                        className="flex flex-col gap-3 pt-3"
                        data-slot="about-update-progress"
                        aria-live="polite"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-2 text-sm tabular-nums">
                          <span>
                            {filesize(snapshot?.downloaded ?? 0, {
                              standard: 'iec',
                            })}
                            {snapshot?.total != null
                              ? ` / ${filesize(snapshot.total, { standard: 'iec' })}`
                              : ` · ${m.settings_about_update_total_unknown()}`}
                          </span>
                          <span>
                            {filesize(snapshot?.speed ?? 0, {
                              standard: 'iec',
                            })}
                            /s
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
                    </motion.div>
                  )}

                  {phase === 'verifying' && (
                    <motion.div
                      key="verifying"
                      className="overflow-hidden"
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={transition}
                    >
                      <div className="pt-3">
                        <LinearProgress className="w-full" indeterminate />
                      </div>
                    </motion.div>
                  )}

                  {phase === 'installing' && (
                    <motion.div
                      key="installing"
                      className="overflow-hidden"
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={transition}
                    >
                      <p
                        className="text-on-surface-variant px-1 pt-3 text-sm"
                        aria-live="polite"
                      >
                        {m.settings_about_update_installing_description()}
                      </p>
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        {!showStatus && snapshot?.error && (
          <p className="text-error px-1 text-sm" role="alert">
            {snapshot.error}
          </p>
        )}

        {snapshot?.supported === false && (
          <p className="text-on-surface-variant px-1 text-sm">
            {m.settings_about_update_unsupported()}
          </p>
        )}
      </SettingsCardContent>
      <SettingsCardFooter
        className="flex-row gap-0 pt-0"
        data-slot="about-update-actions"
      >
        <UpdateActionButton
          variant={phase === 'downloading' ? 'stroked' : 'flat'}
          className="h-auto min-h-10 min-w-0 flex-1 px-4 py-2"
          disabled={canUpdate && (isPending || isLoading || isTransitioning)}
          onClick={action.handleClick}
          label={action.label}
          lastCheckedLabel={lastCheckedLabel}
          loading={canUpdate && (isLoading || isPending || isTransitioning)}
        />
      </SettingsCardFooter>
    </>
  )
}
