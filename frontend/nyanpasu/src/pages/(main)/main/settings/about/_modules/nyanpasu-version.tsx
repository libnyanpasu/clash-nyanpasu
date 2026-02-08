import { PropsWithChildren, useEffect, useState } from 'react'
import Markdown from 'react-markdown'
import AnimatedLogo from '@/components/logo/animated-logo'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { LinearProgress } from '@/components/ui/progress'
import { ScrollArea } from '@/components/ui/scroll-area'
import { SwitchItem } from '@/components/ui/switch'
import { useLockFn } from '@/hooks/use-lock-fn'
import {
  Action as AboutAction,
  Route as AboutRoute,
} from '@/pages/(main)/main/settings/about/route'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { commands, useSetting } from '@nyanpasu/interface'
import { relaunch } from '@tauri-apps/plugin-process'

const TITLE = 'Clash Nyanpasu~(∠・ω< )⌒☆'

const GITHUB_RELEASES_URL =
  'https://github.com/libnyanpasu/clash-nyanpasu/releases'

const AutoCheckUpdate = () => {
  const { value, upsert, isPending } = useSetting('enable_auto_check_update')

  return (
    <SwitchItem
      className="rounded-[20px]"
      checked={value ?? true}
      onCheckedChange={(checked) => upsert(checked)}
      loading={isPending}
    >
      <p className="truncate">{m.settings_label_about_auto_check_updates()}</p>
    </SwitchItem>
  )
}

const NewVersionModal = ({ children }: PropsWithChildren) => {
  const { action } = AboutRoute.useSearch()

  const { newVersion } = useNyanpasuUpdate()

  const [isInstalling, setIsInstalling] = useState(false)

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
      await newVersion.download((e) => {
        switch (e.event) {
          case 'Started':
            setContentLength(e.data.contentLength || 0)
            break
          case 'Progress':
            setContentDownloaded((prev) => prev + e.data.chunkLength)
            break
        }
      })

      await commands.cleanupProcesses()
      // cleanup and stop core
      await newVersion.install()
      // On macOS and Linux you will need to restart the app manually.
      // You could use this step to display another confirmation dialog.
      await relaunch()
    } catch (e) {
      console.error(e)
      message(formatError(e), {
        kind: 'error',
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
            <ScrollArea className="max-h-[calc(100vh-200px)]">
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
    <Card className="space-y-2">
      <CardContent className="items-center">
        <div className="p-4">
          <AnimatedLogo className="size-32" indeterminate />
        </div>

        <div className="truncate text-base font-bold">{TITLE}</div>

        <div className="text-sm font-semibold">
          {m.settings_label_about_version({
            version: currentVersion,
          })}
        </div>
      </CardContent>

      {isSupported ? (
        <CardFooter className="flex-col gap-2">
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
            >
              {m.settings_label_about_update()}
            </Button>
          )}
        </CardFooter>
      ) : (
        <CardFooter>
          <Button
            variant="flat"
            className="w-full"
            onClick={handleUpdateToGithubReleases}
          >
            {m.settings_label_about_update_to_github_releases()}
          </Button>
        </CardFooter>
      )}
    </Card>
  )
}
