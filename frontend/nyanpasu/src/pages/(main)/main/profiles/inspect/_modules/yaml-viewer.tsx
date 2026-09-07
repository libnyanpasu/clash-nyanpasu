import { useEffect, useState } from 'react'
import styles from './yaml-viewer.module.scss'

export default function YamlViewer({
  code,
  label,
}: {
  code: string
  label: string
}) {
  const [highlighted, setHighlighted] = useState<{
    code: string
    html: string
  }>()

  useEffect(() => {
    let cancelled = false
    import('@/utils/shiki')
      .then(({ highlightYaml }) => highlightYaml(code))
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
      role="region"
      aria-label={label}
      tabIndex={0}
      className={`${styles.viewer} bg-surface text-on-surface focus-visible:outline-primary h-[55vh] min-h-64 overflow-auto rounded-lg text-xs focus-visible:outline-2`}
    >
      {highlighted?.code === code ? (
        <div
          className="h-full"
          dangerouslySetInnerHTML={{ __html: highlighted.html }}
        />
      ) : (
        <pre>
          <code>{code}</code>
        </pre>
      )}
    </div>
  )
}
