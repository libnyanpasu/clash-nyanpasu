import { useCreation } from 'ahooks'
import { useAtomValue } from 'jotai'
import { nanoid } from 'nanoid'
import { lazy, Suspense } from 'react'
import { useTranslation } from 'react-i18next'
import { themeMode } from '@/store'
import {
  useProfile,
  useProfileContent,
  useRuntimeProfile,
} from '@nyanpasu/interface'
import { BaseDialog, cn } from '@nyanpasu/ui'

const MonacoDiffEditor = lazy(() => import('./profile-monaco-diff-viewer'))

export type RuntimeConfigDiffDialogProps = {
  open: boolean
  onClose: () => void
}

export default function RuntimeConfigDiffDialog({
  open,
  onClose,
}: RuntimeConfigDiffDialogProps) {
  const { t } = useTranslation()

  const { query } = useProfile()

  const currentProfileUid = query.data?.current?.[0]

  const contentFn = useProfileContent(currentProfileUid || '')

  // need manual refetch
  contentFn.query.refetch()

  const runtimeProfile = useRuntimeProfile()

  const loaded = !contentFn.query.isLoading && !query.isLoading

  const mode = useAtomValue(themeMode)

  const originalModelPath = useCreation(() => `${nanoid()}.clash.yaml`, [])
  const modifiedModelPath = useCreation(() => `${nanoid()}.runtime.yaml`, [])

  if (!currentProfileUid) {
    return null
  }

  return (
    <BaseDialog title={t('Runtime Config')} open={open} onClose={onClose}>
      <div className="xs:w-[95vw] h-full w-[80vw] px-4">
        <div
          className={cn(
            'items-center justify-between px-5 pb-2',
            loaded ? 'flex' : 'hidden',
          )}
        >
          <span className="text-base font-semibold">
            {t('Original Config')}
          </span>
          <span className="text-base font-semibold">{t('Runtime Config')}</span>
        </div>
        <div className="h-[75vh] w-full">
          <Suspense fallback={null}>
            {loaded && (
              <MonacoDiffEditor
                language="yaml"
                theme={mode === 'light' ? 'vs' : 'vs-dark'}
                original={contentFn.query.data}
                originalModelPath={originalModelPath}
                modified={runtimeProfile.data}
                modifiedModelPath={modifiedModelPath}
                options={{
                  minimap: { enabled: false },
                  automaticLayout: true,
                  readOnly: true,
                }}
              />
            )}
          </Suspense>
        </div>
      </div>
    </BaseDialog>
  )
}
