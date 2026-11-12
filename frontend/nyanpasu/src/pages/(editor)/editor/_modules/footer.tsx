import { ComponentProps } from 'react'

export function EditorFooter(props: ComponentProps<'div'>) {
  return (
    <div
      className="bg-background flex min-h-12 shrink-0 flex-wrap items-center gap-2 px-3 py-2"
      data-slot="editor-footer-actions"
      {...props}
    />
  )
}
