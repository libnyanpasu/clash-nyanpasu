import dayjs from 'dayjs'
import { useAtomValue, useSetAtom } from 'jotai'
import { useEffect } from 'react'
import { atomEnableLog, atomLogData } from '@/store'
import { LogMessage, useClashWS } from '@nyanpasu/interface'

const MAX_LOG_NUM = 1000

export const LogProvider = () => {
  const {
    logs: { latestMessage },
  } = useClashWS()

  const setLogData = useSetAtom(atomLogData)

  const enableLog = useAtomValue(atomEnableLog)

  useEffect(() => {
    if (!latestMessage?.data || !enableLog) {
      return
    }

    const data = JSON.parse(latestMessage?.data) as LogMessage
    const time = dayjs(data.time).format('MM-DD HH:mm:ss')
    setLogData((prev) => {
      if (prev.length >= MAX_LOG_NUM) {
        prev.shift()
      }
      return [...prev, { ...data, time }]
    })
  }, [enableLog, latestMessage?.data, setLogData])

  return null
}

export default LogProvider
