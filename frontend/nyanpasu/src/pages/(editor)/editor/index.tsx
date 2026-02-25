import { isEqual } from 'lodash-es'
import { editor } from 'monaco-editor'
import { ComponentProps, useCallback, useEffect, useRef, useState } from 'react'
import { z } from 'zod'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import MonacoEditor from '@monaco-editor/react'
import { openThat, useProfileContent } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { ask, message } from '@tauri-apps/plugin-dialog'
import Chip from './_modules/chip'
import Header from './_modules/header'
import { useCurrentProfile } from './_modules/hooks'
import LoadingSkeleton from './_modules/loading-skeleton'
import { beforeEditorMount, MONACO_FONT_FAMILY } from './_modules/utils'

const currentWindow = getCurrentWebviewWindow()

export const Route = createFileRoute('/(editor)/editor/')({
  component: RouteComponent,
  validateSearch: z.object({
    uid: z.string(),
  }),
})

const ActionButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => {
  return <Button className={cn('h-8 min-w-0 px-3', className)} {...props} />
}

function RouteComponent() {
  const { themeMode } = useExperimentalThemeContext()

  const { uid } = Route.useSearch()

  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)

  const editorMarks = useRef<editor.IMarker[]>([])

  const currentProfile = useCurrentProfile(uid)

  const content = useProfileContent(uid)

  const [editorValue, setEditorValue] = useState<string>()

  // sync editor value with content
  useEffect(() => {
    if (content.query.data) {
      setEditorValue(content.query.data)
    }
  }, [content.query.data])

  const blockTask = useBlockTask(`save-profile-content-${uid}`, async () => {
    if (!editorValue) {
      return
    }

    const editorHasError =
      editorMarks.current.length > 0 &&
      editorMarks.current.some((m) => m.severity === 8)

    if (editorHasError) {
      message(m.editor_validate_error_message(), {
        kind: 'error',
      })

      return false
    }

    await content.upsert.mutateAsync(editorValue)

    await currentWindow.close()
  })

  const handleSave = useLockFn(blockTask.execute)

  const handleBeforeClose = useLockFn(async () => {
    const isDirty = !isEqual(editorValue, content.query.data)

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

    await currentWindow.close()
  })

  const handleReset = useLockFn(async () => {
    setEditorValue(content.query.data)
  })

  const handleEditorDidMount = useCallback(
    (editor: editor.IStandaloneCodeEditor) => {
      editorRef.current = editor

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
              openThat(url)
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
      <Header beforeClose={handleBeforeClose} />

      {content.query.isLoading || currentProfile.isLoading ? (
        <LoadingSkeleton />
      ) : (
        <>
          <div
            className={cn(
              'dark:bg-on-primary bg-primary-container flex shrink-0 items-center gap-2 px-3',
              'h-12',
            )}
            data-slot="editor-header-actions"
          >
            <div
              className="text-sm font-medium"
              data-slot="editor-header-title"
            >
              {currentProfile.data?.name}.{currentProfile.data?.extension}
            </div>

            {currentProfile.data?.readOnly && (
              <Chip>{m.editor_read_only_chip()}</Chip>
            )}
          </div>

          <div className="min-h-0 flex-1" data-slot="editor-content">
            <MonacoEditor
              className="h-full w-full"
              value={content.query.data}
              language={currentProfile.data?.language}
              path={currentProfile.data?.virtualPath}
              theme={themeMode === 'light' ? 'vs' : 'vs-dark'}
              beforeMount={beforeEditorMount}
              onMount={handleEditorDidMount}
              onChange={setEditorValue}
              onValidate={(marks) => {
                editorMarks.current = marks
              }}
              loading={<LoadingSkeleton />}
              options={{
                readOnly: currentProfile.data?.readOnly,
                mouseWheelZoom: true,
                renderValidationDecorations: 'on',
                tabSize: currentProfile.data?.language === 'yaml' ? 2 : 4,
                minimap: { enabled: false },
                automaticLayout: true,
                fontLigatures: true,
                smoothScrolling: true,
                fontFamily: MONACO_FONT_FAMILY,
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
            <ActionButton onClick={handleReset}>
              {m.common_reset()}
            </ActionButton>

            <div className="flex-1" />

            <ActionButton onClick={handleCancel}>
              {m.common_cancel()}
            </ActionButton>

            <ActionButton
              className="px-5"
              variant="flat"
              loading={blockTask.isPending}
              onClick={handleSave}
            >
              {m.common_save()}
            </ActionButton>
          </div>
        </>
      )}
    </>
  )
}
