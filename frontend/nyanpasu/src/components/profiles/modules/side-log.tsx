import { isEmpty } from 'lodash-es'
import { memo } from 'react'
import { useTranslation } from 'react-i18next'
import { VList } from 'virtua'
import { RamenDining, Terminal } from '@mui/icons-material'
import { Divider } from '@mui/material'
import { useClash, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { filterProfiles } from '../utils'

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

  // const { getRuntimeLogs, getProfiles } = useClash()

  const { query } = useProfile()

  const { chain } = filterProfiles(query.data?.items)

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
        {/* {!isEmpty(getRuntimeLogs.data) ? (
          Object.entries(getRuntimeLogs.data).map(([uid, content]) => {
            return content.map((item, index) => {
              const name = scripts?.find((script) => script.uid === uid)?.name

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
        ) : ( */}
        <div className="flex h-full min-h-48 w-full flex-col items-center justify-center">
          <RamenDining className="!size-10" />
          <p>{t('No Logs')}</p>
        </div>
        {/* )} */}
      </VList>
    </div>
  )
}
