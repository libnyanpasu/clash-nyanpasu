import { PropsWithChildren, useEffect, useState } from 'react'
import Markdown from 'react-markdown'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@nyanpasu/ui/modal'
import { LinearProgress } from '@nyanpasu/ui/progress'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { SwitchItem } from '@nyanpasu/ui/switch'
import AnimatedLogo from '@/components/logo/animated-logo'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import {
  Action as AboutAction,
  Route as AboutRoute,
} from '@/pages/(main)/main/settings/about/route'
import { m } from '@/paraglide/messages'
import { commands } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting } from '@nyanpasu/query'
import { type ReleaseChannel } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { relaunch } from '@tauri-apps/plugin-process'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card'
import UpdateSourceSelector from './update-source-selector'

const TITLE = 'Clash Nyanpasu~(∠・ω< )⌒☆'

const GITHUB_RELEASES_URL =
  'https://github.com/libnyanpasu/clash-nyanpasu/releases'

const AutoCheckUpdate = () => {
  const { value, upsert, isPending } = useSetting('enable_auto_check_update')
  const { isInstalling, isChecking } = useNyanpasuUpdate()

  return (
    <SwitchItem
      className="rounded-[20px]"
      checked={value ?? true}
      onCheckedChange={(checked) => upsert(checked)}
      loading={isPending}
      disabled={isPending || isInstalling || isChecking}
    >
      <p className="truncate">{m.settings_label_about_auto_check_updates()}</p>
    </SwitchItem>
  )
}

const ReleaseChannelSelector = () => {
  const {
    releaseChannel,
    isChannelLocked,
    setReleaseChannel,
    isChangingChannel,
    isChecking,
    isInstalling,
  } = useNyanpasuUpdate()
  const labels: Record<ReleaseChannel, string> = {
    stable: m.release_channel_stable(),
    beta: m.release_channel_beta(),
    nightly: m.release_channel_nightly(),
  }
  const notices: Partial<Record<ReleaseChannel, string>> = {
    stable: m.release_channel_stable_notice(),
    nightly: m.release_channel_nightly_notice(),
  }
  const handleChange = useLockFn(async (channel: ReleaseChannel) => {
    try {
      await setReleaseChannel(channel)
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    }
  })
  return (
    <div
      className={cn(
        'flex w-full items-center justify-between gap-4',
        'bg-surface-variant/30 dark:bg-surface-variant/10',
        'min-h-16 rounded-[20px] px-4 py-3',
      )}
    >
      <div className="flex min-w-0 flex-col gap-0.5">
        <p className="truncate">{m.release_channel_label()}</p>

        {/* a nightly build locks the selector, so explain why inline */}
        {isChannelLocked && (
          <p className="text-on-surface-variant text-xs">{notices.nightly}</p>
        )}
      </div>

      <Select
        variant="outlined"
        value={releaseChannel ?? ''}
        onValueChange={(value) => handleChange(value as ReleaseChannel)}
        disabled={
          !releaseChannel ||
          isChannelLocked ||
          isChecking ||
          isChangingChannel ||
          isInstalling
        }
      >
        <SelectTrigger
          className="h-10 w-32 flex-none py-2 data-disabled:cursor-not-allowed data-disabled:opacity-50"
          aria-label={m.release_channel_label()}
        >
          <SelectValue className="truncate pr-4 text-sm">
            {releaseChannel ? labels[releaseChannel] : null}
          </SelectValue>
        </SelectTrigger>

        <SelectContent align="end" className="min-w-72">
          {(Object.keys(labels) as ReleaseChannel[]).map((channel) => (
            <SelectItem
              key={channel}
              value={channel}
              textValue={labels[channel]}
              className="h-auto min-h-12 py-3"
            >
              <span className="flex flex-col gap-0.5">
                <span>{labels[channel]}</span>

                {notices[channel] && (
                  <span className="text-on-surface-variant text-xs">
                    {notices[channel]}
                  </span>
                )}
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

const NewVersionModal = ({ children }: PropsWithChildren) => {
  const { action } = AboutRoute.useSearch()

  const { newVersion, downloadNewVersion, isInstalling, setIsInstalling } =
    useNyanpasuUpdate()

  const [contentLength, setContentLength] = useState(0)
  const [contentDownloaded, setContentDownloaded] = useState(0)

  const progress =
    contentDownloaded && contentLength
      ? (contentDownloaded / contentLength) * 100
      : 0

  const [open, setOpen] = useState(false)

  useEffect(() => {
    // for animation duration to open the modal
    if (action === AboutAction.NEED_UPDATE) {
      setOpen(true)
    }
  }, [action])

  const handleOpenChange = (open: boolean) => {
    if (isInstalling) {
      return
    }

    setOpen(open)
  }

  // const newVersionReleasesPageUrl = IS_NIGHTLY
  //   ? `https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/pre-release`
  //   : `https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/v${newVersion?.version}`

  const handleUpdate = useLockFn(async () => {
    if (!newVersion) {
      return
    }

    try {
      setIsInstalling(true)

      // Install the update. This will also restart the app on Windows!
      setContentDownloaded(0)
      setContentLength(0)
      const downloaded = await downloadNewVersion((e) => {
        switch (e.event) {
          case 'Started':
            setContentDownloaded(0)
            setContentLength(e.data.contentLength || 0)
            break
          case 'Progress':
            setContentDownloaded((prev) => prev + e.data.chunkLength)
            break
        }
      })

      await commands.cleanupProcesses()
      // cleanup and stop core
      await downloaded.install()
      // On macOS and Linux you will need to restart the app manually.
      // You could use this step to display another confirmation dialog.
      await relaunch()
    } catch (e) {
      console.error(e)
      message(formatError(e), {
        kind: 'error',
        error: e,
        title: 'Error',
      })
    } finally {
      setIsInstalling(false)
    }
  })

  return (
    <Modal open={open} onOpenChange={handleOpenChange}>
      <ModalTrigger asChild>{children}</ModalTrigger>

      <ModalContent>
        <Card className="max-w-3xl min-w-96">
          <CardHeader>
            <ModalTitle>
              {m.settings_label_about_update_has_new_version()}
            </ModalTitle>
          </CardHeader>

          <CardContent asChild>
            <ScrollArea className="max-h-[80dvh]">
              {isInstalling ? (
                <div className="flex flex-col gap-2">
                  <div className="flex items-center gap-2">
                    {m.settings_label_about_update_installing()}

                    <span className="text-xs text-slate-500">
                      {progress.toFixed(2)}%
                    </span>
                  </div>

                  <LinearProgress className="w-full" value={progress} />
                </div>
              ) : (
                <Markdown
                  components={{
                    a(props) {
                      const { children, node, ...rest } = props

                      return (
                        <a
                          {...rest}
                          onClick={(e) => {
                            e.preventDefault()
                            e.stopPropagation()

                            if (typeof node?.properties.href === 'string') {
                              commands.openThat(node.properties.href)
                            }
                          }}
                        >
                          {children}
                        </a>
                      )
                    },
                  }}
                >
                  {newVersion?.body || 'New version available.'}
                </Markdown>
              )}
            </ScrollArea>
          </CardContent>

          <CardFooter className="gap-2">
            <Button
              variant="flat"
              loading={isInstalling}
              disabled={!newVersion || isInstalling}
              onClick={handleUpdate}
            >
              {m.settings_label_about_update_to_update_button()}
            </Button>

            {!isInstalling && <ModalClose>{m.common_close()}</ModalClose>}
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}

export default function NyanpasuVersion() {
  const {
    currentVersion,
    hasNewVersion,
    isChecking,
    checkNewVersion,
    isSupported,
    isInstalling,
  } = useNyanpasuUpdate()

  const handleUpdateToGithubReleases = useLockFn(
    async () => await commands.openThat(GITHUB_RELEASES_URL),
  )

  const handleCheckNewVersion = useLockFn(async () => {
    const update = await checkNewVersion()

    if (update) {
      message(m.settings_label_about_update_has_new_version(), {
        kind: 'info',
        title: m.settings_label_about_update(),
      })
    } else {
      message(m.settings_label_about_update_no_update(), {
        kind: 'info',
        title: m.settings_label_about_update(),
      })
    }
  })

  return (
    <SettingsCard className="space-y-2">
      <SettingsCardContent className="items-center gap-4">
        <div className="p-4">
          <AnimatedLogo className="size-32" indeterminate />
        </div>

        <div className="truncate text-base font-bold">{TITLE}</div>

        <div className="text-sm font-semibold">
          {m.settings_label_about_version({
            version: currentVersion,
          })}
        </div>
      </SettingsCardContent>

      {isSupported ? (
        <SettingsCardFooter className="flex-col gap-2">
          <ReleaseChannelSelector />

          <UpdateSourceSelector />

          <AutoCheckUpdate />

          {hasNewVersion ? (
            <NewVersionModal>
              <Button variant="flat" className="w-full">
                {m.settings_label_about_update_has_new_version()}
              </Button>
            </NewVersionModal>
          ) : (
            <Button
              variant="flat"
              className="w-full"
              onClick={handleCheckNewVersion}
              loading={isChecking}
              disabled={isInstalling}
            >
              {m.settings_label_about_update()}
            </Button>
          )}
        </SettingsCardFooter>
      ) : (
        <SettingsCardFooter className="flex-col gap-2">
          <ReleaseChannelSelector />

          <Button
            variant="flat"
            className="w-full"
            onClick={handleUpdateToGithubReleases}
          >
            {m.settings_label_about_update_to_github_releases()}
          </Button>
        </SettingsCardFooter>
      )}
    </SettingsCard>
  )
}
