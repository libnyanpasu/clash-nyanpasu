import { useState, type FormEvent } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Input } from '@nyanpasu/ui/input'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { useNyanpasuUpdate } from '@/components/providers/nyanpasu-update-provider'
import { useAppUpdateChangelog } from '@nyanpasu/query'
import { AppUpdateChangelog } from '../../_modules/app-update-changelog'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardHeader,
} from '../../../_modules/settings-card'

type VersionRange = { from: string; to: string }

function ChangelogSources({ from, to }: VersionRange) {
  const { data } = useAppUpdateChangelog(from, to)

  if (!data) return null

  return (
    <div
      className="text-on-surface-variant font-mono text-xs select-text"
      data-slot="changelog-preview-sources"
    >
      {data.entries.length > 0
        ? data.entries
            .map((entry) => `${entry.version} · ${entry.source}`)
            .join('\n')
        : 'No entries'}
    </div>
  )
}

/** Renders the update dialog's release notes for any two versions. */
export default function ChangelogPreview() {
  const { currentVersion } = useNyanpasuUpdate()
  const [draft, setDraft] = useState<VersionRange>({
    from: currentVersion,
    to: '',
  })
  const [range, setRange] = useState<VersionRange>()

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault()
    setRange({ from: draft.from.trim(), to: draft.to.trim() })
  }

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>Changelog Preview</SettingsCardHeader>

        <SettingsCardContent className="flex flex-col gap-3">
          <form
            className="flex flex-wrap items-center gap-2"
            data-slot="changelog-preview-form"
            onSubmit={handleSubmit}
          >
            <Input
              variant="outlined"
              label="From (installed)"
              value={draft.from}
              onChange={(event) =>
                setDraft({ ...draft, from: event.target.value })
              }
            />

            <Input
              variant="outlined"
              label="To (offered)"
              value={draft.to}
              onChange={(event) =>
                setDraft({ ...draft, to: event.target.value })
              }
            />

            <Button type="submit" variant="flat" disabled={!draft.to.trim()}>
              Render
            </Button>
          </form>

          {range && (
            <>
              <ChangelogSources {...range} />

              <ScrollArea
                className="border-outline-variant max-h-[60dvh] rounded-xl border"
                data-slot="changelog-preview-result"
              >
                <div className="p-4">
                  <AppUpdateChangelog from={range.from} to={range.to} />
                </div>
              </ScrollArea>
            </>
          )}
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
