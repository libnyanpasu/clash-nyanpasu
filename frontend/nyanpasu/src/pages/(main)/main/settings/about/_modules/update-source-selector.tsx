import { useState } from 'react'
import { SwitchItem } from '@nyanpasu/ui/switch'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider, KeyboardSensor, PointerSensor } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import { useLockFn } from '@nyanpasu/hooks'
import { useSetting } from '@nyanpasu/query'
import { type UpdateSource } from '@nyanpasu/rpc/types'
import { isTauri } from '@tauri-apps/api/core'
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardHeader,
} from '../../_modules/settings-card'

const SOURCES: UpdateSource[] = ['nyanpasu', 'github', 'ghfast', 'sourceforge']

const SOURCE_HOSTS: Record<UpdateSource, string> = {
  nyanpasu: 'majokeiko.com',
  github: 'github.com',
  ghfast: 'ghfast.top',
  sourceforge: 'sourceforge.net',
}

// Without a drag handle, the default mouse activation requires a press-and-hold.
const SENSORS = [
  PointerSensor.configure({
    activationConstraints: (event, source) => {
      if (event.pointerType === 'mouse') return undefined

      const defaults = PointerSensor.defaults.activationConstraints

      return typeof defaults === 'function' ? defaults(event, source) : defaults
    },
  }),
  KeyboardSensor,
]

function SortableSource({
  source,
  index,
  label,
  disabled,
  onToggle,
}: {
  source: UpdateSource
  index: number
  label: string
  disabled: boolean
  onToggle: (checked: boolean) => void
}) {
  const { ref, isDragging } = useSortable({
    id: source,
    index,
    disabled,
  })

  return (
    <Tooltip delayDuration={600}>
      <TooltipTrigger asChild>
        <div
          ref={ref}
          tabIndex={disabled ? -1 : 0}
          aria-label={`${m.update_sources_reorder()} ${label}`}
          className={`bg-surface-variant/30 dark:bg-surface-variant/10 flex min-h-16 items-center gap-2 rounded-[20px] p-4 ${disabled ? '' : 'cursor-grab'} ${isDragging ? 'opacity-60' : ''}`}
          data-slot="about-package-source-item"
          data-source={source}
        >
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="truncate text-sm">{label}</span>
            <span className="text-on-surface-variant truncate text-xs">
              {SOURCE_HOSTS[source]} · {index + 1}
            </span>
          </div>
          <SwitchItem
            className="h-auto w-auto shrink-0 bg-transparent p-0 dark:bg-transparent"
            checked
            aria-label={label}
            disabled={disabled}
            onCheckedChange={onToggle}
          />
        </div>
      </TooltipTrigger>

      <TooltipContent>{m.update_sources_reorder()}</TooltipContent>
    </Tooltip>
  )
}

function FixedSource({
  source,
  label,
  disabled,
  onToggle,
}: {
  source: UpdateSource
  label: string
  disabled: boolean
  onToggle: (checked: boolean) => void
}) {
  return (
    <div
      className="bg-surface-variant/30 dark:bg-surface-variant/10 flex min-h-16 items-center gap-2 rounded-[20px] p-4"
      data-slot="about-package-source-item"
      data-source={source}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-sm">{label}</span>
        <span className="text-on-surface-variant truncate text-xs">
          {SOURCE_HOSTS[source]}
        </span>
      </div>
      <SwitchItem
        className="h-auto w-auto shrink-0 bg-transparent p-0 dark:bg-transparent"
        checked={false}
        aria-label={label}
        disabled={disabled}
        onCheckedChange={onToggle}
      />
    </div>
  )
}

export default function UpdateSourceSelector() {
  if (!isTauri()) {
    return (
      <SettingsCard data-slot="about-package-source-card">
        <SettingsCardHeader>
          <h2 className="text-base font-semibold">
            {m.update_sources_title()}
          </h2>
        </SettingsCardHeader>
        <SettingsCardContent className="pt-2">
          <p className="text-on-surface-variant text-sm">
            {m.settings_about_update_desktop_only()}
          </p>
        </SettingsCardContent>
      </SettingsCard>
    )
  }

  return <UpdateSourceSelectorControls />
}

function UpdateSourceSelectorControls() {
  const { value, upsert, isPending, refetch } = useSetting('update_sources')
  const { isBusy } = useNyanpasuUpdate()
  const [isSaving, setIsSaving] = useState(false)
  const isDisabled = !value || isPending || isSaving || isBusy
  const enabledSources = value?.length ? value : SOURCES
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

  const labels: Record<UpdateSource, string> = {
    nyanpasu: m.update_source_nyanpasu(),
    github: m.update_source_github(),
    ghfast: m.update_source_ghfast(),
    sourceforge: m.update_source_sourceforge(),
  }

  return (
    <SettingsCard data-slot="about-package-source-card">
      <SettingsCardHeader>
        <h2 className="text-base font-semibold">{m.update_sources_title()}</h2>
      </SettingsCardHeader>
      <SettingsCardContent className="gap-3 pt-2">
        <p className="text-on-surface-variant text-sm">
          {m.update_sources_description()}
        </p>
        <DragDropProvider
          sensors={SENSORS}
          onDragEnd={async (event) => {
            if (isDisabled || event.canceled) return
            const next = move(enabledSources, event)
            if (
              next.some((source, index) => source !== enabledSources[index])
            ) {
              await saveSources(next)
            }
          }}
        >
          <div className="flex flex-col gap-3">
            {enabledSources.map((source, index) => (
              <SortableSource
                key={source}
                source={source}
                index={index}
                label={labels[source]}
                disabled={isDisabled || enabledSources.length === 1}
                onToggle={(checked) => toggleSource(source, checked)}
              />
            ))}
            {SOURCES.filter((source) => !enabledSources.includes(source)).map(
              (source) => (
                <FixedSource
                  key={source}
                  source={source}
                  label={labels[source]}
                  disabled={isDisabled}
                  onToggle={(checked) => toggleSource(source, checked)}
                />
              ),
            )}
          </div>
        </DragDropProvider>
      </SettingsCardContent>
    </SettingsCard>
  )
}
