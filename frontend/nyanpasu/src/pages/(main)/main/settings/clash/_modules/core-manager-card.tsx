import ArrowRightAltRounded from '~icons/material-symbols/arrow-right-alt-rounded'
import DeployedCodeUpdateOutlineRounded from '~icons/material-symbols/deployed-code-update-outline-rounded'
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import { filesize } from 'filesize'
import { AnimatePresence, motion } from 'framer-motion'
import { isObject } from 'lodash-es'
import { useMemo, useState } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { CircularProgress } from '@/components/ui/progress'
import TextMarquee from '@/components/ui/text-marquee'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import useCoreIcon from '@/hooks/use-core-icon'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  ClashCore,
  ClashCoresDetail,
  InspectUpdater,
  inspectUpdater,
  useClashConnections,
  useClashCores,
  useSetting,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

function useCoreUpdateTask(
  core?: ClashCore | null,
  item?: ClashCoresDetail | null,
) {
  const { query, updateCore } = useClashCores()

  const [updater, setUpdater] = useState<InspectUpdater>()

  const task = useBlockTask(`core-manager-update-${core}`, async () => {
    try {
      const updaterId = await updateCore.mutateAsync(core!)

      if (!updaterId) {
        throw new Error('Failed to update')
      }

      await new Promise<void>((resolve, reject) => {
        const interval = setInterval(async () => {
          const result = await inspectUpdater(updaterId)

          setUpdater(result)

          if (
            isObject(result.downloader.state) &&
            Object.prototype.hasOwnProperty.call(
              result.downloader.state,
              'failed',
            )
          ) {
            reject(result.downloader.state.failed)
            clearInterval(interval)
          }

          if (result.state === 'done') {
            resolve()
            clearInterval(interval)
          }
        }, 100)
      })

      await query.refetch()

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
      return `${filesize(downloaded)} / ${filesize(total)} · ${filesize(speed)}/s`
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

const CoreItem = ({
  core,
  item,
  onClick,
}: {
  core: ClashCore
  item: ClashCoresDetail
  onClick: (core: ClashCore) => void
}) => {
  const { value: currentCore } = useSetting('clash_core')

  const icon = useCoreIcon(core)

  const isSelected = core === currentCore

  const haveNewVersion = item.latestVersion
    ? item.latestVersion !== item.currentVersion
    : false

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
      className={cn(
        'relative h-auto w-full min-w-0 overflow-hidden rounded-2xl p-2 text-left',
        'flex items-center gap-2',
      )}
      onClick={() => {
        if (updateCoreTask.isPending) {
          return
        }

        onClick(core)
      }}
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

      {haveNewVersion && (
        <div className="m-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                className="size-8"
                variant="stroked"
                icon
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

            <TooltipContent>
              {m.settings_clash_core_manager_card_click_to_update()}
            </TooltipContent>
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

  const { deleteConnections } = useClashConnections()

  const { value: currentCoreKey } = useSetting('clash_core')

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
        title: 'Error',
      })
    }
  })

  const isLoading =
    clashCores.isPending ||
    switchCoreTask.isPending ||
    restartSidecarTask.isPending

  const loadingMessage = m.settings_clash_core_manager_card_loading()

  const haveNewVersion = currentCore?.latestVersion
    ? currentCore.latestVersion !== currentCore.currentVersion
    : false

  const currentCoreUpdate = useCoreUpdateTask(currentCoreKey, currentCore)

  return (
    <SettingsCard data-slot="core-manager-card">
      <SettingsCardContent
        data-slot="core-manager-card-content"
        className="flex flex-col gap-3 px-2"
      >
        <Card className="relative">
          <AnimatePresence initial={false}>
            {isLoading && (
              <motion.div
                data-slot="core-manager-card-mask"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className={cn(
                  'bg-primary/10 absolute inset-0 z-50 backdrop-blur-3xl',
                  'flex flex-col items-center justify-center gap-4',
                )}
              >
                <CircularProgress className="size-12" indeterminate />

                <p>{loadingMessage}</p>
              </motion.div>
            )}
          </AnimatePresence>

          <CardHeader className="px-5">
            {m.settings_clash_core_manager_card_title()}
          </CardHeader>

          <CardContent>
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
                <p className="font-medium">{currentCore?.name}</p>

                <p className="flex items-center gap-1 text-sm">
                  {currentCoreUpdate.task.isPending &&
                  currentCoreUpdate.stateLabel ? (
                    <span className="text-emerald-700">
                      {currentCoreUpdate.stateLabel}
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
                {haveNewVersion && (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="stroked"
                        icon
                        onClick={() => currentCoreUpdate.task.execute()}
                        loading={currentCoreUpdate.task.isPending}
                      >
                        <DeployedCodeUpdateOutlineRounded className="size-5" />
                      </Button>
                    </TooltipTrigger>

                    <TooltipContent>
                      {m.settings_clash_core_manager_card_click_to_update()}
                    </TooltipContent>
                  </Tooltip>
                )}

                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      icon
                      variant="stroked"
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
                    onClick={() => switchCoreTask.execute(core as ClashCore)}
                  />
                )
              })}
            </div>
          </CardContent>

          <CardFooter className="gap-2">
            <Button
              variant="flat"
              onClick={handleFetchRemote}
              loading={fetchRemote.isPending}
            >
              {m.settings_clash_core_manager_card_fetch_remote()}
            </Button>
          </CardFooter>
        </Card>
      </SettingsCardContent>
    </SettingsCard>
  )
}
