import { useEffect, useMemo, useRef } from 'react'
import { useColorScheme } from '@/hooks/use-color-scheme'
import { cn } from '@nyanpasu/utils'
import { useQuery, type QueryKey } from '@tanstack/react-query'
import { useVirtualizer } from '@tanstack/react-virtual'
import { revealWithin, type FindRequest } from './find-bar'
import { findLines, type FieldJump } from './text-find'
import { tokenStyle } from './token-style'

// Shiki highlights on the main thread in one task that grows with the
// document (about 0.85 s for 22k lines), and the page shows a step again on
// every view switch. Larger documents stay plain text.
const MAX_HIGHLIGHT_LINES = 4000

// Every line has the same height, so the lines and the highlight bars sit at
// fixed offsets and only the lines in view are mounted. A document of
// thousands of lines mounted in full took a layout of 100 ms and more.
const LINE_HEIGHT = 20
const PADDING = 12
// Lines mounted beyond each edge of the view, so scrolling shows no gap.
const OVERSCAN = 20
// The line number gutter, and the room after the longest line.
const GUTTER_WIDTH = 52
const END_PADDING = 16
// A search that matches nearly every line of a huge document draws this many.
const MAX_BARS = 1000

// The line numbers are drawn from each line's `data-line`, so they are not part
// of the text that is selected, copied or searched.
const viewerClassName = cn(
  'bg-code text-on-surface focus-visible:outline-primary min-h-0 flex-1 overflow-auto',
  'font-mono text-[12.5px] leading-5 focus-visible:-outline-offset-2 focus-visible:outline-2',
  '[&_.line]:before:content-[attr(data-line)]',
  '[&_.line]:before:text-tc [&_.line]:before:inline-block [&_.line]:before:w-[52px]',
  '[&_.line]:before:pr-[18px] [&_.line]:before:text-right [&_.line]:before:select-none',
)

const barTop = (line: number) => PADDING + line * LINE_HEIGHT

/** The columns a line takes in a monospace font; a wide character takes two. */
function columns(line: string) {
  let count = 0
  for (const char of line) {
    count += char.codePointAt(0)! >= 0x1100 ? 2 : 1
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
  const lines = useMemo(() => code.split('\n'), [code])
  const highlight = lines.length <= MAX_HIGHLIGHT_LINES
  // The lines are absolutely placed, so the longest one sets the width.
  const width = useMemo(
    () => lines.reduce((widest, line) => Math.max(widest, columns(line)), 0),
    [lines],
  )

  const scheme = useColorScheme()
  const { data: tokens } = useQuery({
    queryKey: ['inspect-yaml-tokens', scheme, ...cacheKey],
    queryFn: async () => {
      const { tokenizeYaml } = await import('@/utils/shiki')
      return tokenizeYaml(code, scheme)
    },
    enabled: highlight,
    staleTime: Infinity,
    gcTime: 60_000,
    retry: false,
  })

  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => regionRef.current,
    estimateSize: () => LINE_HEIGHT,
    paddingStart: PADDING,
    paddingEnd: PADDING,
    overscan: OVERSCAN,
  })

  const matches = useMemo(
    () => (find?.query ? findLines(lines, find.query) : []),
    [lines, find?.query],
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
      className={viewerClassName}
    >
      <div
        className="relative"
        style={{
          height: virtualizer.getTotalSize(),
          minWidth: `calc(${GUTTER_WIDTH + END_PADDING}px + ${width}ch)`,
        }}
      >
        {matches.slice(0, MAX_BARS).map((line) => (
          <Bar key={line} line={line} active={line === activeLine} />
        ))}
        {activeLine !== undefined && find!.active >= MAX_BARS && (
          <Bar line={activeLine} active />
        )}
        {jump && <Bar line={jump.start} jump />}

        <pre className="m-0" data-highlighted={tokens ? '' : undefined}>
          <code>
            {virtualizer.getVirtualItems().map(({ index, start }) => (
              <span
                key={index}
                className="line absolute inset-x-0 block h-5 whitespace-pre"
                style={{ top: start }}
                data-line={index + 1}
              >
                {tokens?.[index]?.map((token, tokenIndex) => (
                  <span key={tokenIndex} style={tokenStyle(token)}>
                    {token.content}
                  </span>
                )) ?? lines[index]}
              </span>
            ))}
          </code>
        </pre>
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
