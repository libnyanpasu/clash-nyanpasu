import {
  createContext,
  PropsWithChildren,
  use,
  useEffect,
  useState,
} from 'react'
import { Action as AboutAction } from '@/pages/(main)/main/settings/about/route'
import {
  commands,
  unwrapResult,
  useSetting,
  useUpdaterSupported,
} from '@nyanpasu/interface'
import packageJson from '@root/package.json'
import { useNavigate } from '@tanstack/react-router'
import { Update } from '@tauri-apps/plugin-updater'
import { useBlockTask } from './block-task-provider'

const NyanpasuUpdateContext = createContext<{
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

  const isSupported = useUpdaterSupported()

  const [hasNewVersion, setHasNewVersion] = useState(false)

  const [newVersion, setNewVersion] = useState<Update | null>(null)

  const blockTask = useBlockTask('check-nyanpasu-update', async () => {
    const metadata = unwrapResult(await commands.checkUpdate())

    if (metadata) {
      const update = new Update({
        rid: metadata.rid,
        currentVersion: metadata.current_version,
        version: metadata.version,
        rawJson: metadata.raw_json as Record<string, unknown>,
      })

      setNewVersion(update)

      setHasNewVersion(true)

      return update
    }

    return null
  })

  const navigate = useNavigate()

  // auto check update
  useEffect(() => {
    if (enableAutoCheckUpdate) {
      blockTask.execute().then((update) => {
        // if there is a new version, navigate to the about page
        if (update) {
          navigate({
            to: '/main/settings/about',
            search: {
              action: AboutAction.NEED_UPDATE,
            },
          })
        }
      })
    }
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
  }, [enableAutoCheckUpdate, blockTask.execute])

  return (
    <NyanpasuUpdateContext.Provider
      value={{
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
