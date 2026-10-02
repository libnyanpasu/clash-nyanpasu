import {
  createContext,
  PropsWithChildren,
  use,
  useEffect,
  useRef,
  useState,
} from 'react'
import { rpc } from '@/services/rpc'
import { isUpdaterSupported } from '@/utils/updater-support'
import { isLinux } from '@nyanpasu/platform'
import {
  useIsAppImage,
  useIsPortable,
  useReleaseChannel,
  useSetting,
} from '@nyanpasu/query'
import { unwrapResult } from '@nyanpasu/rpc'
import {
  type ReleaseChannel,
  type UpdateDownloadEvent,
  type UpdateSource,
} from '@nyanpasu/rpc/types'
import packageJson from '@root/package.json'
import { Channel, isTauri } from '@tauri-apps/api/core'
import { Update } from '@tauri-apps/plugin-updater'
import { useBlockTask } from './block-task-provider'

const NyanpasuUpdateContext = createContext<{
  releaseChannel: ReleaseChannel | undefined
  /** A nightly build cannot leave the Nightly channel. */
  isChannelLocked: boolean
  setReleaseChannel: (channel: ReleaseChannel) => Promise<void>
  isChangingChannel: boolean
  currentVersion: string
  hasNewVersion: boolean
  newVersion: Update | null
  isChecking: boolean
  isInstalling: boolean
  setIsInstalling: (installing: boolean) => void
  checkNewVersion: () => Promise<Update | null>
  /** Resolves only when the backend could not install the update. */
  installNewVersion: (
    onEvent?: (event: UpdateDownloadEvent) => void,
  ) => Promise<void>
  isSupported: boolean
} | null>(null)

export const useNyanpasuUpdate = () => {
  const context = use(NyanpasuUpdateContext)

  if (!context) {
    throw new Error(
      'useNyanpasuUpdate must be used within a NyanpasuUpdateProvider',
    )
  }

  return context
}

export default function NyanpasuUpdateProvider({
  children,
}: PropsWithChildren) {
  const { value: enableAutoCheckUpdate } = useSetting(
    'enable_auto_check_update',
  )
  const { value: updateSources } = useSetting('update_sources')

  const { query: channelQuery, mutation: channelMutation } = useReleaseChannel()
  const releaseChannel = channelQuery.data?.current
  const channelRef = useRef(releaseChannel)
  channelRef.current = releaseChannel
  const configKey = JSON.stringify([releaseChannel, updateSources])
  const configRef = useRef({ key: configKey, generation: 0 })
  if (configRef.current.key !== configKey) {
    configRef.current = {
      key: configKey,
      generation: configRef.current.generation + 1,
    }
  }

  const { data: isAppImage } = useIsAppImage()
  const { data: isPortable } = useIsPortable()

  const isSupported = isUpdaterSupported({
    tauri: isTauri(),
    linux: isLinux,
    appImage: isAppImage,
    portable: isPortable,
  })

  const [downloads, setDownloads] = useState<{
    generation: number
    candidates: { source: UpdateSource; update: Update }[]
  } | null>(null)
  const newVersion = downloads?.candidates[0]?.update ?? null
  const hasNewVersion = newVersion !== null
  const [isInstalling, setIsInstalling] = useState(false)
  const lastAutoCheckKey = useRef<string | null>(null)

  const blockTask = useBlockTask('check-nyanpasu-update', async () => {
    if (
      !isSupported ||
      !updateSources?.length ||
      !releaseChannel ||
      isInstalling
    )
      return null
    const checkedChannel = channelRef.current
    const checkedGeneration = configRef.current.generation
    const metadata = unwrapResult(await rpc.checkUpdate())
    const candidates =
      metadata?.downloads.map(({ source, rid }) => ({
        source,
        update: new Update({
          rid,
          currentVersion: metadata.current_version,
          version: metadata.version,
          date: metadata.date ?? undefined,
          body: metadata.body ?? undefined,
          rawJson: metadata.raw_json as Record<string, unknown>,
        }),
      })) ?? []

    if (
      checkedChannel !== channelRef.current ||
      checkedGeneration !== configRef.current.generation
    ) {
      await Promise.all(candidates.map(({ update }) => update.close()))
      return null
    }

    if (metadata) {
      setDownloads({ generation: checkedGeneration, candidates })
      return candidates[0]?.update ?? null
    }

    setDownloads(null)
    return null
  })

  const setReleaseChannel = async (channel: ReleaseChannel) => {
    await channelMutation.mutateAsync(channel)
    channelRef.current = channel
    configRef.current.generation += 1
    setDownloads(null)
  }

  useEffect(() => {
    if (
      !isInstalling &&
      downloads?.generation !== configRef.current.generation
    ) {
      setDownloads(null)
    }
  }, [configKey, isInstalling, downloads])

  useEffect(
    () => () => {
      downloads?.candidates.forEach(({ update }) => {
        update.close().catch(console.error)
      })
    },
    [downloads],
  )

  // The backend downloads from each source in turn, shuts the app down and
  // hands over to the installer, so this settles only on failure.
  const installNewVersion = async (
    onEvent?: (event: UpdateDownloadEvent) => void,
  ) => {
    const channel = new Channel<UpdateDownloadEvent>()
    channel.onmessage = (event) => onEvent?.(event)

    unwrapResult(
      await rpc.installUpdate(
        (downloads?.candidates ?? []).map(({ source, update }) => ({
          source,
          rid: update.rid,
        })),
        channel,
      ),
    )
  }

  // auto check update
  useEffect(() => {
    const key = JSON.stringify([
      isSupported,
      enableAutoCheckUpdate,
      configRef.current.generation,
    ])
    if (
      isSupported &&
      enableAutoCheckUpdate &&
      releaseChannel &&
      updateSources?.length &&
      !isInstalling &&
      !blockTask.isPending &&
      lastAutoCheckKey.current !== key
    ) {
      lastAutoCheckKey.current = key
      blockTask.execute()
    }
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
  }, [
    isSupported,
    enableAutoCheckUpdate,
    releaseChannel,
    updateSources,
    isInstalling,
    blockTask.isPending,
    blockTask.execute,
  ])

  return (
    <NyanpasuUpdateContext.Provider
      value={{
        releaseChannel,
        isChannelLocked: channelQuery.data?.installed === 'nightly',
        setReleaseChannel,
        isChangingChannel: channelMutation.isPending,
        currentVersion: packageJson.version,
        hasNewVersion,
        newVersion,
        isChecking: blockTask.isPending,
        isInstalling,
        setIsInstalling,
        checkNewVersion: blockTask.execute,
        installNewVersion,
        isSupported,
      }}
    >
      {children}
    </NyanpasuUpdateContext.Provider>
  )
}
