import { useMemo } from 'react'
import { useQuery, type QueryKey } from '@tanstack/react-query'
import styles from './yaml-viewer.module.scss'

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
      role="region"
      aria-label={label}
      tabIndex={0}
      className={`${styles.viewer} bg-surface text-on-surface focus-visible:outline-primary h-[55vh] min-h-64 overflow-auto rounded-lg text-xs focus-visible:outline-2 @[40rem]:h-auto @[40rem]:min-h-48 @[40rem]:flex-1`}
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
