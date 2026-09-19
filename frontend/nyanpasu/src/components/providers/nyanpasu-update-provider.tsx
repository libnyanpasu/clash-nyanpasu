import {
  createContext,
  PropsWithChildren,
  use,
  useEffect,
  useRef,
  useState,
} from 'react'
import {
  commands,
  unwrapResult,
  useIsAppImage,
  useReleaseChannel,
  useSetting,
  type ReleaseChannel,
} from '@nyanpasu/interface'
import packageJson from '@root/package.json'
import { Update } from '@tauri-apps/plugin-updater'
import { useBlockTask } from './block-task-provider'

const NyanpasuUpdateContext = createContext<{
  releaseChannel: ReleaseChannel | undefined
  setReleaseChannel: (channel: ReleaseChannel) => Promise<void>
  isChangingChannel: boolean
  currentVersion: string
  hasNewVersion: boolean
  newVersion: Update | null
  isChecking: boolean
  checkNewVersion: () => Promise<Update | null>
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

  const { query: channelQuery, mutation: channelMutation } = useReleaseChannel()
  const releaseChannel = channelQuery.data
  const channelRef = useRef(releaseChannel)
  channelRef.current = releaseChannel

  const { data: isAppImage } = useIsAppImage()

  // windows portable version does not support auto update
  const isSupported = !isAppImage || !WIN_PORTABLE

  const [hasNewVersion, setHasNewVersion] = useState(false)

  const [newVersion, setNewVersion] = useState<Update | null>(null)

  const blockTask = useBlockTask('check-nyanpasu-update', async () => {
    const checkedChannel = channelRef.current
    const metadata = unwrapResult(await commands.checkUpdate())

    if (metadata) {
      const update = new Update({
        rid: metadata.rid,
        currentVersion: metadata.current_version,
        version: metadata.version,
        rawJson: metadata.raw_json as Record<string, unknown>,
      })

      if (checkedChannel !== channelRef.current) {
        await update.close()
        return null
      }
      setNewVersion(update)

      setHasNewVersion(true)

      return update
    }

    setNewVersion(null)
    setHasNewVersion(false)
    return null
  })

  const setReleaseChannel = async (channel: ReleaseChannel) => {
    await channelMutation.mutateAsync(channel)
    channelRef.current = channel
    setNewVersion(null)
    setHasNewVersion(false)
  }

  useEffect(() => {
    setNewVersion(null)
    setHasNewVersion(false)
  }, [releaseChannel])

  useEffect(
    () => () => {
      newVersion?.close().catch(console.error)
    },
    [newVersion],
  )

  // auto check update
  useEffect(() => {
    if (enableAutoCheckUpdate && releaseChannel) {
      blockTask.execute()
    }
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
  }, [enableAutoCheckUpdate, releaseChannel, blockTask.execute])

  return (
    <NyanpasuUpdateContext.Provider
      value={{
        releaseChannel,
        setReleaseChannel,
        isChangingChannel: channelMutation.isPending,
        currentVersion: packageJson.version,
        hasNewVersion,
        newVersion,
        isChecking: blockTask.isPending,
        checkNewVersion: blockTask.execute,
        isSupported,
      }}
    >
      {children}
    </NyanpasuUpdateContext.Provider>
  )
}
