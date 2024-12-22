import { useDebounceEffect } from 'ahooks'
import { useAtom, useAtomValue, useSetAtom } from 'jotai'
import { useState } from 'react'
import { atomLogData } from '@/store'
import { LogFilter } from './log-filter'
import { LogLevel } from './log-level'
import LogToggle from './log-toggle'
import { atomLogLevel, atomLogList } from './modules/store'

export const LogHeader = () => {
  const [logState, setLogState] = useAtom(atomLogLevel)

  const [filterText, setFilterText] = useState('')

  const logData = useAtomValue(atomLogData)

  const setLogList = useSetAtom(atomLogList)

  useDebounceEffect(
    () => {
      setLogList(
        logData.filter((data) => {
          return (
            data.payload.includes(filterText) &&
            (logState === 'all' ? true : data.type.includes(logState))
          )
        }),
      )
    },
    [logData, logState, filterText],
    { wait: 150 },
  )

  return (
    <div className="flex gap-2">
      <LogToggle />

      <LogLevel value={logState} onChange={(value) => setLogState(value)} />

      <LogFilter
        value={filterText}
        onChange={(value) => setFilterText(value)}
      />
    </div>
  )
}

export default LogHeader
