import { cva } from 'class-variance-authority'
import { useEffect, useState } from 'react'
import Markdown, { type Components, type Options } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkGithub from 'remark-github'
import { cn } from '@nyanpasu/utils'
import { openExternal } from './open-external'

const REMARK_PLUGINS = [
  remarkGfm,
  // Links @mentions, #issues and commit hashes the way GitHub renders them.
  [remarkGithub, { repository: 'libnyanpasu/clash-nyanpasu' }],
] satisfies Options['remarkPlugins']

function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const [highlighted, setHighlighted] = useState<{
    code: string
    html: string
  }>()

  useEffect(() => {
    if (!lang) return
    let cancelled = false
    import('@/utils/shiki')
      .then(({ highlightMarkdownCode }) => highlightMarkdownCode(code, lang))
      .then((html) => {
        if (!cancelled && html) setHighlighted({ code, html })
      })
      .catch(() => {
        if (!cancelled) setHighlighted(undefined)
      })
    return () => {
      cancelled = true
    }
  }, [code, lang])

  return (
    <div
      data-slot="changelog-markdown-code-block"
      className="bg-surface-variant/30 my-3 overflow-x-auto rounded-xl p-3 text-xs [&_.shiki]:!bg-transparent dark:[&_.shiki]:!text-[var(--shiki-dark)] [&_.shiki_span]:!bg-transparent dark:[&_.shiki_span]:!text-[var(--shiki-dark)] [&_pre]:m-0 [&_pre]:font-mono [&_pre]:leading-relaxed"
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

const components: Components = {
  a({ href, title, children }) {
    const external = href != null && /^(https?|mailto):/i.test(href)

    return (
      <a
        href={href}
        title={title}
        onClick={(event) => {
          event.preventDefault()
          if (external) openExternal(href)
        }}
      >
        {children}
      </a>
    )
  },
  pre({ node }) {
    const code = node?.children[0]
    if (code?.type !== 'element' || code.tagName !== 'code') return null
    const text = code.children
      .map((child) => (child.type === 'text' ? child.value : ''))
      .join('')
      .replace(/\n$/, '')
    const lang = (code.properties.className as string[] | undefined)
      ?.find((name) => name.startsWith('language-'))
      ?.slice('language-'.length)

    return <CodeBlock code={text} lang={lang} />
  },
}

// Body headings sit below the version heading each entry renders. Changelog
// list items are separated by blank lines, which wraps every item in a
// paragraph; `li > p` keeps them as compact as a tight list.
const changelogMarkdownVariants = cva([
  'text-on-surface text-sm leading-relaxed wrap-anywhere select-text',
  '[&_a]:text-primary [&_a]:underline-offset-2 [&_a:hover]:underline',
  '[&_h1]:mt-5 [&_h1]:mb-2 [&_h1]:text-lg [&_h1]:font-semibold',
  '[&_h2]:mt-5 [&_h2]:mb-2 [&_h2]:text-base [&_h2]:font-semibold',
  '[&_h3]:mt-4 [&_h3]:mb-1.5 [&_h3]:text-sm [&_h3]:font-semibold',
  '[&_h4]:mt-3 [&_h4]:mb-1 [&_h4]:text-sm [&_h4]:font-medium',
  '[&>:first-child]:mt-0 [&_p]:my-2',
  '[&_ul]:my-2 [&_ul]:list-disc [&_ul]:space-y-1 [&_ul]:pl-5',
  '[&_ol]:my-2 [&_ol]:list-decimal [&_ol]:space-y-1 [&_ol]:pl-5',
  '[&_li]:pl-0.5 [&_li>p]:my-0',
  // Like GitHub, only task items drop the marker; the checkbox takes its place.
  // The unlayered normalize reset zeroes input margins, hence the `!`.
  '[&_.task-list-item]:list-none [&_.task-list-item_input]:-ml-5! [&_.task-list-item_input]:mr-1.5!',
  '[&_input]:accent-primary [&_input]:align-middle dark:[&_input]:[color-scheme:dark]',
  '[&_:not(pre)>code]:bg-surface-variant/50 [&_:not(pre)>code]:rounded [&_:not(pre)>code]:px-1 [&_:not(pre)>code]:py-0.5 [&_:not(pre)>code]:text-[0.9em]',
  '[&_blockquote]:border-outline-variant [&_blockquote]:text-on-surface-variant [&_blockquote]:my-3 [&_blockquote]:border-l-4 [&_blockquote]:pl-3',
  '[&_hr]:border-outline-variant [&_hr]:my-4',
  '[&_table]:my-3 [&_table]:block [&_table]:max-w-full [&_table]:overflow-x-auto [&_table]:border-collapse [&_table]:text-left',
  '[&_td]:border-outline-variant [&_td]:border [&_td]:px-2 [&_td]:py-1',
  '[&_th]:border-outline-variant [&_th]:bg-surface-variant/30 [&_th]:border [&_th]:px-2 [&_th]:py-1 [&_th]:font-semibold',
  '[&_img]:my-2 [&_img]:inline-block [&_img]:max-w-full [&_img]:rounded-lg',
])

/** Renders GitHub-flavored release notes; links open in the system browser. */
export function ChangelogMarkdown({
  className,
  children,
}: {
  className?: string
  children: string
}) {
  return (
    <div
      data-slot="changelog-markdown"
      className={cn(changelogMarkdownVariants(), className)}
    >
      <Markdown remarkPlugins={REMARK_PLUGINS} components={components}>
        {children}
      </Markdown>
    </div>
  )
}
