import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded'
import { Button } from '@nyanpasu/ui/button'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { useAppUpdateChangelog } from '@nyanpasu/query'
import type { AppUpdateChangelogEntry } from '@nyanpasu/rpc/types'
import { ChangelogMarkdown } from './changelog-markdown'
import { openExternal } from './open-external'

function ChangelogEntry({ entry }: { entry: AppUpdateChangelogEntry }) {
  return (
    <section data-slot="app-update-changelog-entry">
      <header
        className="border-outline-variant mb-2 flex items-baseline gap-2 border-b pb-1.5"
        data-slot="app-update-changelog-entry-header"
      >
        <h3 className="text-base font-semibold">v{entry.version}</h3>

        {entry.date && (
          <time
            className="text-on-surface-variant text-xs"
            dateTime={entry.date}
          >
            {entry.date}
          </time>
        )}
      </header>

      <ChangelogMarkdown>{entry.body}</ChangelogMarkdown>
    </section>
  )
}

/**
 * Release notes of every version after `from` up to `to`. When none can be
 * resolved, `fallbackBody` (the offered release's own notes) stands in.
 */
export function AppUpdateChangelog({
  from,
  to,
  fallbackBody,
}: {
  from: string
  to: string
  fallbackBody?: string | null
}) {
  const { data, error, isPending } = useAppUpdateChangelog(from, to)

  if (isPending) {
    return (
      <div
        className="text-on-surface-variant flex items-center gap-2 py-6 text-sm"
        data-slot="app-update-changelog-loading"
      >
        <CircularProgress className="size-4" indeterminate />
        {m.settings_about_update_changelog_loading()}
      </div>
    )
  }

  const entries = data?.entries ?? []
  const compareUrl = data?.compare_url

  return (
    <div className="flex flex-col gap-6" data-slot="app-update-changelog">
      {error && (
        <p className="text-error text-sm">
          {m.settings_about_update_changelog_failed({
            error: formatError(error),
          })}
        </p>
      )}

      {entries.map((entry) => (
        <ChangelogEntry key={entry.version} entry={entry} />
      ))}

      {entries.length === 0 && fallbackBody && (
        <ChangelogMarkdown>{fallbackBody}</ChangelogMarkdown>
      )}

      {compareUrl && (
        <Button
          className="-ml-2 flex items-center gap-1.5 self-start"
          variant="basic"
          onClick={() => openExternal(compareUrl)}
        >
          <OpenInNewRounded className="size-4" />
          {m.settings_about_update_changelog_compare()}
        </Button>
      )}
    </div>
  )
}
