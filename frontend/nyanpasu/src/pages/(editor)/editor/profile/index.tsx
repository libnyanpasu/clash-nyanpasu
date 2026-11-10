import { editor } from 'monaco-editor'
import { useCallback, useState } from 'react'
import z from 'zod'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { ask, message } from '@/utils/notification'
import MonacoEditor from '@monaco-editor/react'
import { useLockFn } from '@nyanpasu/hooks'
import { createWindowAdapter } from '@nyanpasu/platform'
import { usePlatform } from '@nyanpasu/platform/react'
import { useProfileContent } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import ActionButton from '../_modules/action-button'
import Chip from '../_modules/chip'
import Header from '../_modules/header'
import { useCurrentProfile, useDraft } from '../_modules/hooks'
import LoadingSkeleton from '../_modules/loading-skeleton'
import { EDITOR_SHORTCUTS, useShortcuts } from '../_modules/shortcuts'
import { EditorStatus, hasErrors } from '../_modules/status'
import {
  EditorToolbar,
  EditorTools,
  ShortcutTooltip,
} from '../_modules/toolbar'
import { useMonacoTheme } from '../_modules/use-monaco-theme'
import { beforeEditorMount, getMonacoFontFamily } from '../_modules/utils'

const LANGUAGE_LABELS: Record<string, string> = {
  yaml: 'YAML',
  javascript: 'JavaScript',
  lua: 'Lua',
}

export const Route = createFileRoute('/(editor)/editor/profile/')({
  component: RouteComponent,
  validateSearch: z.object({
    uid: z.string(),
  }),
})

function RouteComponent() {
  const { clientOs } = usePlatform()
  const [currentWindow] = useState(createWindowAdapter)
  const { uid } = Route.useSearch()

  const monacoTheme = useMonacoTheme()

  const [editorInstance, setEditorInstance] =
    useState<editor.IStandaloneCodeEditor | null>(null)
  const [markers, setMarkers] = useState<editor.IMarker[]>([])
  const [wordWrap, setWordWrap] = useState(false)

  const currentProfile = useCurrentProfile(uid)
  const readOnly = currentProfile.data?.readOnly ?? false

  const content = useProfileContent(uid)

  const [editorValue, setEditorValue] = useDraft(content.query.data)
  const isDirty =
    editorValue !== undefined &&
    content.query.data !== undefined &&
    editorValue !== content.query.data

  const saveTask = useBlockTask(
    `save-profile-content-${uid}`,
    async (close: boolean) => {
      if (!editorValue || readOnly) {
        return
      }

      if (hasErrors(markers)) {
        message(m.editor_validate_error_message(), {
          kind: 'error',
        })

        return
      }

      await content.upsert.mutateAsync(editorValue)

      if (close) {
        await currentWindow?.close()
      }
    },
  )

  const handleSave = useLockFn(saveTask.execute)

  const handleBeforeClose = useLockFn(async () => {
    if (isDirty) {
      const result = await ask(m.editor_before_close_message(), {
        kind: 'warning',
      })

      if (!result) {
        return false
      }
    }

    return true
  })

  const handleCancel = useLockFn(async () => {
    const result = await handleBeforeClose()

    if (!result) {
      return
    }

    await currentWindow?.close()
  })

  const handleReset = useLockFn(async () => {
    setEditorValue(content.query.data)
  })

  useShortcuts([
    [EDITOR_SHORTCUTS.apply, () => handleSave(false)],
    [EDITOR_SHORTCUTS.save, () => handleSave(true)],
    [EDITOR_SHORTCUTS.close, handleCancel],
    [EDITOR_SHORTCUTS.wordWrap, () => setWordWrap((wordWrap) => !wordWrap)],
  ])

  const handleEditorDidMount = useCallback(
    (editor: editor.IStandaloneCodeEditor) => {
      setEditorInstance(editor)

      // Enable URL detection and handling
      editor.onMouseDown((e) => {
        const position = e.target.position
        if (!position) return

        // Get the model
        const model = editor.getModel()
        if (!model) return

        // Get the word at the clicked position
        const wordAtPosition = model.getWordAtPosition(position)
        if (!wordAtPosition) return

        // More comprehensive URL regex pattern
        const urlRegex = /\b(?:https?:\/\/|www\.)[^\s<>"']*[^<>\s"',.!?]/gi

        // Check if the clicked word is part of a URL
        const lineContent = model.getLineContent(position.lineNumber)
        let match

        while ((match = urlRegex.exec(lineContent)) !== null) {
          const urlStart = match.index + 1
          const urlEnd = urlStart + match[0].length

          // Check if the click position is within the URL
          if (position.column >= urlStart && position.column <= urlEnd) {
            // Only handle Ctrl+Click or Cmd+Click
            if (e.event.ctrlKey || e.event.metaKey) {
              // Add protocol if missing (for www. URLs)
              const url = match[0].startsWith('http')
                ? match[0]
                : `https://${match[0]}`
              rpc.openThat(url)
              e.event.preventDefault()
              break
            }
          }
        }
      })
    },
    [],
  )

  return (
    <>
      <Header
        title={
          currentProfile.data &&
          `${currentProfile.data.name}.${currentProfile.data.extension}`
        }
        dirty={isDirty}
        beforeClose={handleBeforeClose}
      />

      {content.query.isLoading || currentProfile.isLoading ? (
        <LoadingSkeleton />
      ) : (
        <>
          <EditorToolbar>
            <div
              className="text-sm font-medium"
              data-slot="editor-header-language"
            >
              {LANGUAGE_LABELS[currentProfile.data?.language ?? '']}
            </div>

            {readOnly && (
              <Chip className="bg-surface/60 dark:bg-primary-container">
                {m.editor_read_only_chip()}
              </Chip>
            )}

            <div className="flex-1" />

            <EditorTools
              editor={editorInstance}
              readOnly={readOnly}
              canFormat={currentProfile.data?.language !== 'lua'}
              wordWrap={wordWrap}
              onWordWrapChange={setWordWrap}
            />
          </EditorToolbar>

          <div className="min-h-0 flex-1" data-slot="editor-content">
            <MonacoEditor
              className="h-full w-full"
              value={editorValue}
              language={currentProfile.data?.language}
              path={currentProfile.data?.virtualPath}
              theme={monacoTheme}
              beforeMount={beforeEditorMount}
              onMount={handleEditorDidMount}
              onChange={setEditorValue}
              onValidate={setMarkers}
              loading={<LoadingSkeleton />}
              options={{
                readOnly,
                wordWrap: wordWrap ? 'on' : 'off',
                mouseWheelZoom: true,
                renderValidationDecorations: 'on',
                tabSize: currentProfile.data?.language === 'yaml' ? 2 : 4,
                minimap: { enabled: false },
                automaticLayout: true,
                fontLigatures: true,
                smoothScrolling: true,
                fontFamily: getMonacoFontFamily(clientOs),
                quickSuggestions: {
                  strings: true,
                  comments: true,
                  other: true,
                },
              }}
            />
          </div>

          <div
            className="bg-background flex h-12 shrink-0 items-center gap-2 px-3"
            data-slot="editor-footer-actions"
          >
            <EditorStatus editor={editorInstance} markers={markers} />

            <div className="flex-1" />

            <ActionButton disabled={!isDirty} onClick={handleReset}>
              {m.common_reset()}
            </ActionButton>

            <ShortcutTooltip shortcut={EDITOR_SHORTCUTS.close}>
              <ActionButton onClick={handleCancel}>
                {m.common_cancel()}
              </ActionButton>
            </ShortcutTooltip>

            <ShortcutTooltip shortcut={EDITOR_SHORTCUTS.apply}>
              <ActionButton
                className="px-5"
                variant="flat"
                disabled={readOnly}
                loading={saveTask.isPending}
                onClick={() => handleSave(false)}
              >
                {m.common_apply()}
              </ActionButton>
            </ShortcutTooltip>

            <ShortcutTooltip shortcut={EDITOR_SHORTCUTS.save}>
              <ActionButton
                className="px-5"
                variant="flat"
                disabled={readOnly}
                loading={saveTask.isPending}
                onClick={() => handleSave(true)}
              >
                {m.common_save()}
              </ActionButton>
            </ShortcutTooltip>
          </div>
        </>
      )}
    </>
  )
}
