import { useAtomValue } from 'jotai'
import { memo, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { VList } from 'virtua'
import { RamenDining, Terminal } from '@mui/icons-material'
import { Divider } from '@mui/material'
import { usePostProcessingOutput, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { atomChainsSelected, atomGlobalChainCurrent } from './store'

const LogListItem = memo(function LogListItem({
  name,
  item,
  showDivider,
}: {
  name?: string
  item?: [string, string]
  showDivider?: boolean
}) {
  return (
    <>
      {showDivider && <Divider />}

      <div className="w-full font-mono break-all">
        <span className="rounded-sm bg-blue-600 px-0.5">{name}</span>
        <span className="text-red-500"> [{item?.[0]}]: </span>
        <span>{item?.[1]}</span>
      </div>
    </>
  )
})

export interface SideLogProps {
  className?: string
}

export const SideLog = ({ className }: SideLogProps) => {
  const { t } = useTranslation()

  const { query } = useProfile()

  const profiles = query.data?.items

  const { data } = usePostProcessingOutput()

  const isGlobalChainCurrent = useAtomValue(atomGlobalChainCurrent)

  const currentProfileUid = useAtomValue(atomChainsSelected)

  const currentLogs = useMemo(() => {
    if (currentProfileUid) {
      return data?.scopes[currentProfileUid]
    }

    if (isGlobalChainCurrent) {
      return data?.scopes.global
    }
  }, [currentProfileUid, data, isGlobalChainCurrent])

  return (
    <div className={cn('w-full', className)}>
      <div className="flex items-center justify-between p-2 pl-4">
        <div className="flex items-center gap-2">
          <Terminal />

          <span>{t('Console')}</span>
        </div>
      </div>

      <Divider />

      <VList className="flex flex-col gap-2 overflow-auto p-2 select-text">
        {currentLogs ? (
          Object.entries(currentLogs).map(([uid, content]) => {
            return content?.map((item, index) => {
              const name = profiles?.find((script) => script.uid === uid)?.name

              return (
                <LogListItem
                  key={uid + index}
                  name={name}
                  item={item}
                  showDivider={index !== 0}
                />
              )
            })
          })
        ) : (
          <div className="flex h-full min-h-48 w-full flex-col items-center justify-center">
            <RamenDining className="!size-10" />
            <p>{t('No Logs')}</p>
          </div>
        )}
      </VList>
    </div>
  )
}
