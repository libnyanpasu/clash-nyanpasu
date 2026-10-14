import ArrowRightAltRounded from '~icons/material-symbols/arrow-right-alt-rounded'
import DeployedCodeUpdateOutlineRounded from '~icons/material-symbols/deployed-code-update-outline-rounded'
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import { isObject } from 'es-toolkit/compat'
import { filesize } from 'filesize'
import { motion } from 'motion/react'
import { useMemo, useState, type ReactNode } from 'react'
import { Button } from '@nyanpasu/ui/button'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import {
  useBlockTask,
  useBlockTaskContext,
} from '@/components/providers/block-task-provider'
import useCoreIcon from '@/hooks/use-core-icon'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import {
  canUpdateCoreVersion,
  ClashCoresDetail,
  hasNewerCoreVersion,
  useClashCores,
  useDeleteClashConnections,
  useSetting,
} from '@nyanpasu/query'
import { ClashCore, UpdaterSummary } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card'

function useCoreUpdateTask(
  core?: ClashCore | null,
  item?: ClashCoresDetail | null,
) {
  const { updateCore, inspectUpdater, refetchVersions } = useClashCores()

  const [updater, setUpdater] = useState<UpdaterSummary>()

  const task = useBlockTask(`core-manager-update-${core}`, async () => {
    try {
      const updaterId = await updateCore.mutateAsync(core!)

      if (!updaterId) {
        throw new Error('Failed to update')
      }

      await new Promise<void>((resolve, reject) => {
        let stopped = false
        const poll = async () => {
          try {
            const result = await inspectUpdater(updaterId)
            setUpdater(result)
            if (isObject(result.state) && 'failed' in result.state) {
              throw new Error(result.state.failed)
            }
            if (
              isObject(result.downloader.state) &&
              'failed' in result.downloader.state
            ) {
              throw new Error(result.downloader.state.failed)
            }
            if (result.state === 'done') {
              stopped = true
              resolve()
              return
            }
          } catch (error) {
            stopped = true
            reject(error)
          }
          if (!stopped) setTimeout(poll, 100)
        }
        poll()
      })

      await refetchVersions()

      message(
        `Successfully updated the core ${item?.name} to ${item?.latestVersion}`,
        {
          kind: 'info',
          title: 'Successful',
        },
      )
    } catch (e) {
      console.error(e)
      message(formatError(e), {
        kind: 'error',
        error: e,
        title: 'Error',
      })
    }
  })

  const progress = useMemo(() => {
    if (!updater || !task.isPending) {
      return 0
    }

    const { downloaded, total } = updater.downloader

    if (total <= 0) {
      return 0
    }
    return Math.min((downloaded / total) * 100, 100)
  }, [updater, task.isPending])

  const stateLabel = useMemo(() => {
    if (!updater || !task.isPending) {
      return null
    }

    const state = updater.state

    if (state === 'downloading') {
      const { downloaded, total, speed } = updater.downloader
      return `${filesize(downloaded, { standard: 'iec' })} / ${filesize(total, { standard: 'iec' })} · ${filesize(speed ?? 0, { standard: 'iec' })}/s`
    }

    if (state === 'decompressing') {
      return m.settings_clash_core_manager_card_decompressing()
    }

    if (state === 'replacing') {
      return m.settings_clash_core_manager_card_replacing()
    }

    if (state === 'restarting') {
      return m.settings_clash_core_manager_card_restarting()
    }

    if (state === 'done') {
      return m.settings_clash_core_manager_card_done()
    }

    return null
  }, [updater, task.isPending])

  return { task, progress, stateLabel }
}

const UpdateProgressBar = ({
  isPending,
  progress,
}: {
  isPending: boolean
  progress: number
}) => {
  if (!isPending) {
    return null
  }

  return (
    <motion.div
      className="bg-primary/10 absolute inset-0 origin-left"
      initial={{ scaleX: 0 }}
      animate={{ scaleX: progress / 100 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
    />
  )
}

// Every core-manager task key starts with this prefix; a pending one locks the card.
const CORE_MANAGER_TASK_PREFIX = 'core-manager-'

const SkeletonLine = ({
  className,
  children,
}: {
  className?: string
  children?: ReactNode
}) => (
  <span
    data-slot="core-manager-skeleton"
    className={cn(
      'bg-surface-variant inline-block animate-pulse rounded-full',
      className,
    )}
  >
    {children}
  </span>
)

const CoreItem = ({
  core,
  item,
  locked,
  onClick,
}: {
  core: ClashCore
  item: ClashCoresDetail
  locked: boolean
  onClick: (core: ClashCore) => void
}) => {
  const { value: currentCore } = useSetting('core')

  const icon = useCoreIcon(core)

  const isSelected = core === currentCore

  const haveNewVersion = hasNewerCoreVersion(
    core,
    item.currentVersion,
    item.latestVersion,
  )
  const canUpdate = canUpdateCoreVersion(
    core,
    item.currentVersion,
    item.latestVersion,
    item.versionReadError,
  )
  const updateActionLabel = item.versionReadError
    ? m.settings_clash_core_manager_card_reinstall_core()
    : m.settings_clash_core_manager_card_click_to_update()

  const {
    task: updateCoreTask,
    progress,
    stateLabel: updaterStateLabel,
  } = useCoreUpdateTask(core, item)

  return (
    <Button
      variant={isSelected ? 'raised' : 'basic'}
      data-selected={isSelected}
      data-downloading={updateCoreTask.isPending}
      disabled={locked}
      className={cn(
        'relative h-auto w-full min-w-0 overflow-hidden rounded-2xl p-2 text-left',
        'flex items-center gap-2',
      )}
      onClick={() => onClick(core)}
    >
      <UpdateProgressBar
        isPending={updateCoreTask.isPending}
        progress={progress}
      />

      <div className="relative size-12">
        <img src={icon} alt={item.name} />
      </div>

      <div className="relative flex min-w-0 flex-1 flex-col gap-1">
        <TextMarquee>{item.name}</TextMarquee>

        <TextMarquee className="text-sm">
          {updateCoreTask.isPending && updaterStateLabel ? (
            <span className="text-emerald-700">{updaterStateLabel}</span>
          ) : item.versionReadError ? (
            <span className="text-error">
              {m.settings_clash_core_manager_card_version_read_error()}
            </span>
          ) : haveNewVersion ? (
            <p className="flex items-center gap-1">
              <span>{item.currentVersion}</span>
              <ArrowRightAltRounded />
              <span className="text-emerald-700">{item.latestVersion}</span>
            </p>
          ) : (
            item.currentVersion
          )}
        </TextMarquee>
      </div>

      {canUpdate && (
        <div className="m-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                className="size-8"
                variant="stroked"
                icon
                aria-label={updateActionLabel}
                disabled={locked}
                onClick={(e) => {
                  e.preventDefault()
                  e.stopPropagation()
                  updateCoreTask.execute()
                }}
                loading={updateCoreTask.isPending}
                asChild
              >
                <span>
                  <DeployedCodeUpdateOutlineRounded />
                </span>
              </Button>
            </TooltipTrigger>

            <TooltipContent>{updateActionLabel}</TooltipContent>
          </Tooltip>
        </div>
      )}
    </Button>
  )
}

export default function CoreManagerCard() {
  const {
    query: clashCores,
    upsert: switchCore,
    restartSidecar,
    fetchRemote,
  } = useClashCores()

  const deleteConnections = useDeleteClashConnections()

  const { value: currentCoreKey } = useSetting('core')

  const currentCoreIcon = useCoreIcon(currentCoreKey)

  const currentCore = currentCoreKey && clashCores.data?.[currentCoreKey]

  const switchCoreTask = useBlockTask(
    'core-manager-switch',
    async (core: ClashCore) => {
      try {
        await deleteConnections.mutateAsync(null)

        await switchCore.mutateAsync(core)

        message(m.settings_clash_core_manager_card_loading_success(), {
          kind: 'info',
          title: 'Successful',
        })
      } catch (e) {
        console.error(e)
        message(
          `${m.settings_clash_core_manager_card_loading_error()} \n${formatError(e)}`,
          {
            kind: 'error',
            error: e,
            title: 'Error',
          },
        )
      }
    },
  )

  const restartSidecarTask = useBlockTask(
    'core-manager-restart-sidecar',
    async () => {
      try {
        await restartSidecar()

        message(m.settings_clash_core_manager_card_restart_sidecar_success(), {
          kind: 'info',
          title: 'Successful',
        })
      } catch (e) {
        console.error(e)
        message(
          `${m.settings_clash_core_manager_card_restart_sidecar_error()} \n${formatError(e)}`,
          {
            kind: 'error',
            error: e,
            title: 'Error',
          },
        )
      }
    },
  )

  const handleFetchRemote = useLockFn(async () => {
    try {
      await fetchRemote.mutateAsync()
    } catch (e) {
      console.error(e)
      message(formatError(e), {
        kind: 'error',
        error: e,
        title: 'Error',
      })
    }
  })

  const { tasks } = useBlockTaskContext()

  const isOperating = Object.values(tasks).some(
    (task) =>
      task.status === 'pending' && task.id.startsWith(CORE_MANAGER_TASK_PREFIX),
  )

  const isLoading = clashCores.isPending || switchCoreTask.isPending

  const isLocked = isOperating || clashCores.isPending || fetchRemote.isPending

  const haveNewVersion = hasNewerCoreVersion(
    currentCoreKey,
    currentCore?.currentVersion,
    currentCore?.latestVersion,
  )
  const canUpdate = canUpdateCoreVersion(
    currentCoreKey,
    currentCore?.currentVersion,
    currentCore?.latestVersion,
    currentCore?.versionReadError,
  )
  const updateActionLabel = currentCore?.versionReadError
    ? m.settings_clash_core_manager_card_reinstall_core()
    : m.settings_clash_core_manager_card_click_to_update()

  const currentCoreUpdate = useCoreUpdateTask(currentCoreKey, currentCore)

  return (
    <SettingsCard data-slot="core-manager-card" aria-busy={isLocked}>
      <SettingsCardContent data-slot="core-manager-card-content">
        <div
          className={cn(
            'relative flex items-center gap-3 overflow-hidden rounded-2xl p-4',
            'bg-surface-variant',
          )}
        >
          <UpdateProgressBar
            isPending={currentCoreUpdate.task.isPending}
            progress={currentCoreUpdate.progress}
          />

          <div className="relative size-12">
            <img
              src={currentCoreIcon}
              alt={currentCore?.name}
              className="size-full"
            />
          </div>

          <div className="relative flex-1">
            <p className="font-medium">
              {isLoading ? (
                <SkeletonLine className="h-5 w-24" />
              ) : (
                currentCore?.name
              )}
            </p>

            <p className="flex items-center gap-1 text-sm">
              {isLoading ? (
                <SkeletonLine className="h-4 w-16">
                  <span className="sr-only">
                    {m.settings_clash_core_manager_card_loading()}
                  </span>
                </SkeletonLine>
              ) : currentCoreUpdate.task.isPending &&
                currentCoreUpdate.stateLabel ? (
                <span className="text-emerald-700">
                  {currentCoreUpdate.stateLabel}
                </span>
              ) : currentCore?.versionReadError ? (
                <span className="text-error">
                  {m.settings_clash_core_manager_card_version_read_error()}
                </span>
              ) : haveNewVersion ? (
                <>
                  <span>{currentCore?.currentVersion}</span>
                  <ArrowRightAltRounded />
                  <span className="text-emerald-700">
                    {currentCore?.latestVersion}
                  </span>
                </>
              ) : (
                currentCore?.currentVersion
              )}
            </p>
          </div>

          <div className="relative mr-2 flex items-center gap-3">
            {canUpdate && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="stroked"
                    icon
                    aria-label={updateActionLabel}
                    disabled={isLocked}
                    onClick={() => currentCoreUpdate.task.execute()}
                    loading={currentCoreUpdate.task.isPending}
                  >
                    <DeployedCodeUpdateOutlineRounded className="size-5" />
                  </Button>
                </TooltipTrigger>

                <TooltipContent>{updateActionLabel}</TooltipContent>
              </Tooltip>
            )}

            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  icon
                  variant="stroked"
                  disabled={isLocked}
                  onClick={() => restartSidecarTask.execute()}
                >
                  <RestartAltRounded className="size-5" />
                </Button>
              </TooltipTrigger>

              <TooltipContent>
                {m.settings_clash_core_manager_card_restart_sidecar()}
              </TooltipContent>
            </Tooltip>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
          {Object.entries(clashCores.data ?? {}).map(([core, item]) => {
            if (core === currentCoreKey) {
              return null
            }

            return (
              <CoreItem
                key={item.name}
                core={core as ClashCore}
                item={item}
                locked={isLocked}
                onClick={() => switchCoreTask.execute(core as ClashCore)}
              />
            )
          })}
        </div>
      </SettingsCardContent>

      <SettingsCardFooter className="gap-2">
        <Button
          variant="flat"
          disabled={isLocked}
          onClick={handleFetchRemote}
          loading={fetchRemote.isPending}
        >
          {m.settings_clash_core_manager_card_fetch_remote()}
        </Button>
      </SettingsCardFooter>
    </SettingsCard>
  )
}
