import { useEffect, useMemo, useRef } from 'react'
import { cn } from '@nyanpasu/utils'
import { useQuery, type QueryKey } from '@tanstack/react-query'
import { revealWithin, type FindRequest } from './find-bar'
import { findLines, type FieldJump } from './text-find'

// Shiki highlights on the main thread in one task that grows with the
// document (about 0.85 s for 22k lines), and the page shows a step again on
// every view switch. Larger documents stay plain text.
const MAX_HIGHLIGHT_LINES = 4000

// The highlight bars sit at a fixed offset, so every line has the same height.
const LINE_HEIGHT = 20
const PADDING = 16
// A search that matches nearly every line of a huge document draws this many.
const MAX_BARS = 1000

const barTop = (line: number) => PADDING + line * LINE_HEIGHT

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
  find,
  jump,
}: {
  code: string
  label: string
  /** Identifies `code` for good: the highlighted output is cached under it. */
  cacheKey: QueryKey
  /** Marks every line that contains the text and scrolls to the current one. */
  find?: FindRequest
  /** Scrolls to a field's line and marks it. */
  jump?: FieldJump
}) {
  const regionRef = useRef<HTMLDivElement>(null)
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

  const matches = useMemo(
    () => (find?.query ? findLines(code.split('\n'), find.query) : []),
    [code, find?.query],
  )
  const activeLine = find ? matches[find.active] : undefined

  useEffect(() => {
    const bar =
      regionRef.current?.querySelector<HTMLElement>('[data-find-active]')
    if (bar) revealWithin(regionRef.current!, bar)
  }, [activeLine, find?.query])

  useEffect(() => {
    const bar = regionRef.current?.querySelector<HTMLElement>('[data-jump]')
    if (bar) revealWithin(regionRef.current!, bar)
  }, [jump?.nonce])

  return (
    <div
      ref={regionRef}
      data-slot="yaml-viewer"
      role="region"
      aria-label={label}
      tabIndex={0}
      className="bg-surface text-on-surface focus-visible:outline-primary min-h-0 flex-1 overflow-auto rounded-lg text-xs focus-visible:outline-2 dark:[&_.shiki]:!bg-[var(--shiki-dark-bg)] dark:[&_.shiki]:!text-[var(--shiki-dark)] dark:[&_.shiki_span]:!bg-[var(--shiki-dark-bg)] dark:[&_.shiki_span]:!text-[var(--shiki-dark)] [&_pre]:m-0 [&_pre]:min-h-full [&_pre]:min-w-fit [&_pre]:p-4 [&_pre]:leading-5"
    >
      <div className="relative h-full min-w-fit">
        {matches.slice(0, MAX_BARS).map((line) => (
          <Bar key={line} line={line} active={line === activeLine} />
        ))}
        {activeLine !== undefined && find!.active >= MAX_BARS && (
          <Bar line={activeLine} active />
        )}
        {jump && <Bar line={jump.start} jump />}

        {highlight && html !== undefined ? (
          <div className="h-full" dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          <pre>
            <code>{code}</code>
          </pre>
        )}
      </div>
    </div>
  )
}

function Bar({
  line,
  active,
  jump,
}: {
  line: number
  active?: boolean
  jump?: boolean
}) {
  return (
    <span
      aria-hidden
      data-find={jump ? undefined : ''}
      data-find-active={active ? '' : undefined}
      data-jump={jump ? '' : undefined}
      data-line={line}
      className={cn(
        'pointer-events-none absolute inset-x-0 z-10',
        jump
          ? 'bg-primary/15 outline-primary/60 outline-1 -outline-offset-1'
          : active
            ? 'bg-yellow-400/45 outline-1 -outline-offset-1 outline-yellow-500'
            : 'bg-yellow-400/20',
      )}
      style={{ top: barTop(line), height: LINE_HEIGHT }}
    />
  )
}
