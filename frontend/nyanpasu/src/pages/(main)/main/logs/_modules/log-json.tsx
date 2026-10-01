import { useEffect, useMemo, useState } from 'react'

export default function LogJson({ raw }: { raw: string }) {
  const code = useMemo(() => {
    try {
      return JSON.stringify(JSON.parse(raw), null, 2)
    } catch {
      return raw
    }
  }, [raw])
  const [highlighted, setHighlighted] = useState<{
    code: string
    html: string
  }>()

  useEffect(() => {
    let cancelled = false
    import('@/utils/shiki')
      .then(({ highlightJson }) => highlightJson(code))
      .then((html) => {
        if (!cancelled) setHighlighted({ code, html })
      })
      .catch(() => {
        if (!cancelled) setHighlighted(undefined)
      })
    return () => {
      cancelled = true
    }
  }, [code])

  return (
    <div
      data-slot="log-json"
      className="bg-surface-variant/30 text-on-surface mt-2 rounded-xl p-3 text-xs [&_.shiki]:!bg-transparent dark:[&_.shiki]:!text-[var(--shiki-dark)] [&_.shiki_span]:!bg-transparent dark:[&_.shiki_span]:!text-[var(--shiki-dark)] [&_pre]:m-0 [&_pre]:font-mono [&_pre]:leading-relaxed [&_pre]:wrap-anywhere [&_pre]:whitespace-pre-wrap"
    >
      {highlighted?.code === code ? (
        <div dangerouslySetInnerHTML={{ __html: highlighted.html }} />
      ) : (
        <pre>
          <code>{code}</code>
        </pre>
      )}
    </div>
  )
}
