import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { SwitchItem } from '@nyanpasu/ui/switch'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting } from '@nyanpasu/query'
import { type UpdateSource } from '@nyanpasu/rpc/types'

const SOURCES: UpdateSource[] = ['nyanpasu', 'github', 'ghfast']

const SOURCE_HOSTS: Record<UpdateSource, string> = {
  nyanpasu: 'nyanpasu-script.majokeiko.com',
  github: 'github.com',
  ghfast: 'ghfast.top',
}

export default function UpdateSourceSelector() {
  const { value, upsert, isPending, refetch } = useSetting('update_sources')
  const { isInstalling, isChecking } = useNyanpasuUpdate()
  const [isSaving, setIsSaving] = useState(false)
  const isDisabled =
    !value || isPending || isSaving || isInstalling || isChecking
  const enabledSources = value?.length ? value : SOURCES
  const displayedSources = [
    ...enabledSources,
    ...SOURCES.filter((source) => !enabledSources.includes(source)),
  ]

  const saveSources = useLockFn(async (sources: UpdateSource[]) => {
    setIsSaving(true)
    try {
      await upsert(sources)
      await refetch()
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
    } finally {
      setIsSaving(false)
    }
  })

  const toggleSource = async (source: UpdateSource, checked: boolean) => {
    if (isDisabled) return

    if (checked) {
      await saveSources([...enabledSources, source])
      return
    }

    if (enabledSources.length > 1) {
      await saveSources(enabledSources.filter((item) => item !== source))
    }
  }

  const moveSource = async (index: number, offset: -1 | 1) => {
    if (isDisabled) return
    const targetIndex = index + offset
    if (targetIndex < 0 || targetIndex >= enabledSources.length) return

    const reordered = [...enabledSources]
    ;[reordered[index], reordered[targetIndex]] = [
      reordered[targetIndex],
      reordered[index],
    ]
    await saveSources(reordered)
  }

  const labels: Record<UpdateSource, string> = {
    nyanpasu: m.update_source_nyanpasu(),
    github: m.update_source_github(),
    ghfast: m.update_source_ghfast(),
  }

  return (
    <div className="flex w-full flex-col gap-2">
      <div className="px-1">
        <p className="text-sm font-semibold">{m.update_sources_title()}</p>
        <p className="text-on-surface-variant text-xs">
          {m.update_sources_description()}
        </p>
      </div>

      {displayedSources.map((source) => {
        const index = enabledSources.indexOf(source)
        const enabled = index >= 0

        return (
          <SwitchItem
            key={source}
            className="min-h-16 rounded-[20px]"
            checked={enabled}
            aria-label={labels[source]}
            disabled={isDisabled || (enabled && enabledSources.length === 1)}
            onCheckedChange={(checked) => toggleSource(source, checked)}
            loading={isSaving}
          >
            <div className="flex min-w-0 flex-1 items-center justify-between gap-3">
              <div className="flex min-w-0 flex-col gap-0.5">
                <span className="truncate text-sm">{labels[source]}</span>
                <span className="text-on-surface-variant truncate text-xs">
                  {SOURCE_HOSTS[source]}
                  {enabled && ` · ${index + 1}`}
                </span>
              </div>
              {enabled && (
                <div className="flex shrink-0 gap-1">
                  <Button
                    variant="basic"
                    className="h-8 min-w-0 px-3"
                    aria-label={`${m.update_sources_move_up()} ${labels[source]}`}
                    disabled={isDisabled || index === 0}
                    onClick={(event) => {
                      event.preventDefault()
                      event.stopPropagation()
                      moveSource(index, -1)
                    }}
                  >
                    ↑
                  </Button>
                  <Button
                    variant="basic"
                    className="h-8 min-w-0 px-3"
                    aria-label={`${m.update_sources_move_down()} ${labels[source]}`}
                    disabled={isDisabled || index === enabledSources.length - 1}
                    onClick={(event) => {
                      event.preventDefault()
                      event.stopPropagation()
                      moveSource(index, 1)
                    }}
                  >
                    ↓
                  </Button>
                </div>
              )}
            </div>
          </SwitchItem>
        )
      })}
    </div>
  )
}
