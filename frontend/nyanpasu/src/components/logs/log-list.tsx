import { useDebounceEffect } from 'ahooks'
import { RefObject, useDeferredValue, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Virtualizer, VirtualizerHandle } from 'virtua'
import { cn } from '@nyanpasu/ui'
import ContentDisplay from '../base/content-display'
import LogItem from './log-item'
import { useLogContext } from './log-provider'

export const LogList = ({
  scrollRef,
}: {
  scrollRef: RefObject<HTMLElement>
}) => {
  const { t } = useTranslation()

  const { logs, logLevel } = useLogContext()

  const virtualizerRef = useRef<VirtualizerHandle>(null)

  const shouldStickToBottom = useRef(true)

  const isFirstScroll = useRef(true)

  useDebounceEffect(
    () => {
      if (shouldStickToBottom && logs?.length) {
        virtualizerRef.current?.scrollToIndex(logs?.length - 1, {
          align: 'end',
          smooth: !isFirstScroll.current,
        })

        isFirstScroll.current = false
      }
    },
    [logs],
    { wait: 100 },
  )

  useEffect(() => {
    isFirstScroll.current = true
  }, [logLevel])

  const handleScroll = (_offset: number) => {
    const end = virtualizerRef.current?.findEndIndex() || 0
    if (end + 1 === logs?.length) {
      shouldStickToBottom.current = true
    } else {
      shouldStickToBottom.current = false
    }
  }

  const deferredLogs = useDeferredValue(logs)

  return deferredLogs?.length ? (
    <Virtualizer
      ref={virtualizerRef}
      scrollRef={scrollRef}
      onScroll={handleScroll}
    >
      {deferredLogs?.map((item, index) => {
        return (
          <LogItem
            key={index}
            className={cn(index !== 0 && 'border-t border-zinc-500')}
            value={item}
          />
        )
      })}
    </Virtualizer>
  ) : (
    <ContentDisplay className="absolute" message={t('No Logs')} />
  )
}
