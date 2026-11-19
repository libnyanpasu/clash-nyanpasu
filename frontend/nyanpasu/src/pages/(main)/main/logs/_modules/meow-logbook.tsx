import { useState } from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import { m } from '@/paraglide/messages'
import { useMeowLogbook } from '@nyanpasu/query'
import type { MeowLogbookEntry } from '@nyanpasu/rpc/types'
import { Button } from '@nyanpasu/ui'
import { useDataPageBottomInset } from '../../_modules/data-page'
import { logPanelVariants, LogRecord } from './log-viewer'
import { LogsLayout } from './logs-layout'

// The API accepts a category prefix, so new events in these categories are included.
const eventCategories = [
  'engine',
  'config',
  'subscription',
  'proxy-provider',
  'rule-provider',
  'geodata',
  'listener',
  'network',
  'dialer-proxy',
  'logbook',
]

export default function MeowLogbook({ core }: { core: 'meow' | 'meow-alpha' }) {
  const [kind, setKind] = useState('all')
  const [history, setHistory] = useState<number[]>([])
  const before = history.at(-1) ?? null
  const logbook = useMeowLogbook(core, {
    before,
    kind: kind === 'all' ? null : kind,
  })
  const page = logbook.data
  const cursor =
    page?.enabled && page.more ? page.entries.at(-1)?.id : undefined
  const unavailable = logbook.isError
    ? m.logs_logbook_unavailable()
    : page === null
      ? m.logs_logbook_unsupported()
      : page?.enabled === false
        ? m.logs_logbook_disabled()
        : undefined

  return (
    <LogsLayout
      source="logbook"
      showLogbook
      search={
        <Select
          variant="chip"
          value={kind}
          onValueChange={(next) => {
            setKind(next)
            setHistory([])
          }}
        >
          <SelectTrigger aria-label={m.logs_logbook_event_type()}>
            <SelectValue>
              {kind === 'all' ? m.logs_logbook_all_events() : kind}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{m.logs_logbook_all_events()}</SelectItem>
            {eventCategories.map((category) => (
              <SelectItem key={category} value={category}>
                {category}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      }
      actions={
        <>
          <Button
            variant="glass"
            disabled={before === null}
            onClick={() => setHistory((previous) => previous.slice(0, -1))}
          >
            {m.logs_logbook_newer()}
          </Button>
          <Button
            variant="glass"
            disabled={cursor === undefined || logbook.isError}
            onClick={() => {
              if (cursor !== undefined)
                setHistory((previous) => [...previous, cursor])
            }}
          >
            {m.logs_load_older()}
          </Button>
        </>
      }
    >
      <div className={logPanelVariants()}>
        {logbook.isPending ? (
          <p className="p-4 text-sm" role="status">
            {m.logs_loading()}
          </p>
        ) : unavailable ? (
          <div className="flex items-center gap-2 p-4 text-sm" role="status">
            <span>{unavailable}</span>
            {logbook.isError && (
              <Button
                disabled={logbook.isFetching}
                onClick={() => logbook.refetch()}
              >
                {m.logs_retry()}
              </Button>
            )}
          </div>
        ) : page?.entries.length ? (
          <LogbookRows key={`${kind}:${before}`} entries={page.entries} />
        ) : (
          <EmptyState message={m.logs_empty_message()} />
        )}
      </div>
    </LogsLayout>
  )
}

function LogbookRows({ entries }: { entries: MeowLogbookEntry[] }) {
  const paddingBottom = useDataPageBottomInset()
  return (
    <ScrollArea className="min-h-0 flex-1">
      <div style={{ paddingBottom }}>
        {entries.map((entry) => (
          <LogRecord
            key={entry.id}
            time={new Date(entry.last ?? entry.time).toLocaleString()}
            level={entry.level}
            target={[
              entry.kind,
              entry.subject,
              entry.count > 1 && `×${entry.count}`,
            ]
              .filter(Boolean)
              .join(' · ')}
            message={entry.message}
            raw={entry}
            search=""
          />
        ))}
      </div>
    </ScrollArea>
  )
}
