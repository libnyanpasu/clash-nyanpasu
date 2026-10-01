import { useMemo } from 'react'
import { useQuery, type QueryKey } from '@tanstack/react-query'

// Shiki highlights on the main thread in one task that grows with the
// document (about 0.85 s for 22k lines), and the page shows a step again on
// every view switch. Larger documents stay plain text.
const MAX_HIGHLIGHT_LINES = 4000

const lineCount = (code: string) => {
  let count = 1
  for (let i = code.indexOf('\n'); i !== -1; i = code.indexOf('\n', i + 1)) {
    count++
  }
  return count
}

export default function YamlViewer({
  code,
  label,
  cacheKey,
}: {
  code: string
  label: string
  /** Identifies `code` for good: the highlighted output is cached under it. */
  cacheKey: QueryKey
}) {
  const highlight = useMemo(
    () => lineCount(code) <= MAX_HIGHLIGHT_LINES,
    [code],
  )

  const { data: html } = useQuery({
    queryKey: ['inspect-yaml-html', ...cacheKey],
    queryFn: async () => {
      const { highlightYaml } = await import('@/utils/shiki')
      return highlightYaml(code)
    },
    enabled: highlight,
    staleTime: Infinity,
    gcTime: 60_000,
    retry: false,
  })

  return (
    <div
      data-slot="yaml-viewer"
      role="region"
      aria-label={label}
      tabIndex={0}
      className="bg-surface text-on-surface focus-visible:outline-primary h-[55vh] min-h-64 overflow-auto rounded-lg text-xs focus-visible:outline-2 @[40rem]:h-auto @[40rem]:min-h-48 @[40rem]:flex-1 dark:[&_.shiki]:!bg-[var(--shiki-dark-bg)] dark:[&_.shiki]:!text-[var(--shiki-dark)] dark:[&_.shiki_span]:!bg-[var(--shiki-dark-bg)] dark:[&_.shiki_span]:!text-[var(--shiki-dark)] [&_pre]:m-0 [&_pre]:min-h-full [&_pre]:min-w-fit [&_pre]:p-4"
    >
      {highlight && html !== undefined ? (
        <div className="h-full" dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <pre>
          <code>{code}</code>
        </pre>
      )}
    </div>
  )
}
