import { useEffect, useMemo, useState } from 'react'
import styles from './log-json.module.scss'

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
      className={`${styles.viewer} bg-surface-variant/30 text-on-surface mt-2 rounded-xl p-3 text-xs`}
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
