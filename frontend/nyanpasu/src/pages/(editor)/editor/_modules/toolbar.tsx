import FormatAlignLeftRounded from '~icons/material-symbols/format-align-left-rounded'
import KeyboardCommandKey from '~icons/material-symbols/keyboard-command-key'
import RedoRounded from '~icons/material-symbols/redo-rounded'
import SearchRounded from '~icons/material-symbols/search-rounded'
import UndoRounded from '~icons/material-symbols/undo-rounded'
import WrapTextRounded from '~icons/material-symbols/wrap-text-rounded'
import type { editor } from 'monaco-editor'
import type { ComponentProps, ReactNode } from 'react'
import { Button, type ButtonProps } from '@nyanpasu/ui/button'
import { Kbd } from '@nyanpasu/ui/kbd'
import { Separator } from '@nyanpasu/ui/separator'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { usePlatform } from '@nyanpasu/platform/react'
import { cn } from '@nyanpasu/utils'
import {
  EDITOR_SHORTCUTS,
  formatShortcut,
  shortcutLabel,
  type Shortcut,
} from './shortcuts'
import { useEditorState } from './use-editor-state'

/** Names the shortcut of the control it wraps, after an optional label. */
export function ShortcutTooltip({
  label,
  shortcut,
  children,
}: {
  label?: string
  shortcut: Shortcut
  children: ReactNode
}) {
  const isMacos = usePlatform().clientOs === 'macos'

  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>

      <TooltipContent
        side="bottom"
        className="flex items-center gap-2"
        data-slot="editor-shortcut-tooltip"
      >
        {label}

        <Kbd>{shortcutLabel(shortcut, isMacos)}</Kbd>
      </TooltipContent>
    </Tooltip>
  )
}

function ToolButton({
  label,
  shortcut,
  className,
  ...props
}: ButtonProps & { label: string; shortcut: Shortcut }) {
  return (
    <ShortcutTooltip label={label} shortcut={shortcut}>
      <Button
        className={cn(
          'hover:bg-surface/50 dark:hover:bg-surface-variant/50 size-8',
          'aria-pressed:bg-surface/70 dark:aria-pressed:bg-surface-variant/70',
          className,
        )}
        icon
        aria-label={label}
        {...props}
      />
    </ShortcutTooltip>
  )
}

/** The row under the title bar: what is being edited, then the tools. */
export function EditorToolbar({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex h-12 shrink-0 items-center gap-2 px-3',
        'bg-primary-container dark:bg-on-primary',
        className,
      )}
      data-slot="editor-header-actions"
      {...props}
    />
  )
}

/** The editing commands of `editor`; Monaco runs them, so its keys work too. */
export function EditorTools({
  editor: instance,
  readOnly = false,
  canFormat = true,
  wordWrap,
  onWordWrapChange,
}: {
  editor: editor.IStandaloneCodeEditor | null
  readOnly?: boolean
  canFormat?: boolean
  wordWrap: boolean
  onWordWrapChange: (wordWrap: boolean) => void
}) {
  const { clientOs } = usePlatform()
  const { canUndo, canRedo } = useEditorState(instance)

  // A toolbar click takes the focus, which the commands need in the editor.
  const run = (action: string) => () => {
    instance?.focus()
    instance?.trigger('toolbar', action, null)
  }

  return (
    <div className="flex items-center gap-1" data-slot="editor-tools">
      <ToolButton
        label={m.editor_toolbar_undo()}
        shortcut={EDITOR_SHORTCUTS.undo}
        disabled={readOnly || !canUndo}
        onClick={run('undo')}
        data-slot="editor-tools-undo"
      >
        <UndoRounded className="size-5" />
      </ToolButton>

      <ToolButton
        label={m.editor_toolbar_redo()}
        shortcut={EDITOR_SHORTCUTS.redo}
        disabled={readOnly || !canRedo}
        onClick={run('redo')}
        data-slot="editor-tools-redo"
      >
        <RedoRounded className="size-5" />
      </ToolButton>

      <Separator
        orientation="vertical"
        className="mx-1 data-[orientation=vertical]:h-5"
      />

      <ToolButton
        label={m.editor_toolbar_find()}
        shortcut={EDITOR_SHORTCUTS.find}
        onClick={run('actions.find')}
        data-slot="editor-tools-find"
      >
        <SearchRounded className="size-5" />
      </ToolButton>

      <ToolButton
        label={m.editor_toolbar_format()}
        shortcut={formatShortcut(clientOs)}
        disabled={readOnly || !canFormat}
        onClick={run('editor.action.formatDocument')}
        data-slot="editor-tools-format"
      >
        <FormatAlignLeftRounded className="size-5" />
      </ToolButton>

      <ToolButton
        label={m.editor_toolbar_word_wrap()}
        shortcut={EDITOR_SHORTCUTS.wordWrap}
        aria-pressed={wordWrap}
        onClick={() => onWordWrapChange(!wordWrap)}
        data-slot="editor-tools-word-wrap"
      >
        <WrapTextRounded className="size-5" />
      </ToolButton>

      <ToolButton
        label={m.editor_toolbar_command_palette()}
        shortcut={EDITOR_SHORTCUTS.commandPalette}
        onClick={run('editor.action.quickCommand')}
        data-slot="editor-tools-command-palette"
      >
        <KeyboardCommandKey className="size-5" />
      </ToolButton>
    </div>
  )
}
