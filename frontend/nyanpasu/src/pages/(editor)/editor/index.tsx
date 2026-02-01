import { isEqual } from 'lodash-es'
import { ComponentProps, useEffect, useState } from 'react'
import { z } from 'zod'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import MonacoEditor from '@monaco-editor/react'
import { useProfileContent } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { ask } from '@tauri-apps/plugin-dialog'
import Header from './_modules/header'
import { useCurrentProfile } from './_modules/hooks'
import LoadingSkeleton from './_modules/loading-skeleton'
import { MONACO_FONT_FAMILY } from './_modules/utils'

const currentWindow = getCurrentWebviewWindow()

export const Route = createFileRoute('/(editor)/editor/')({
  component: RouteComponent,
  validateSearch: z.object({
    uid: z.string(),
    readonly: z.boolean().optional().default(false),
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

  const { uid, readonly } = Route.useSearch()

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

  // loading state
  if (content.query.isLoading || currentProfile.isLoading) {
    return <LoadingSkeleton />
  }

  return (
    <>
      <Header beforeClose={handleBeforeClose} />

      <div
        className={cn(
          'dark:bg-on-primary bg-primary-container flex items-center px-3',
          'h-12',
        )}
        data-slot="editor-header-actions"
      >
        <div className="text-sm font-medium" data-slot="editor-header-title">
          {currentProfile.data?.name}.{currentProfile.data?.extension}
        </div>
      </div>

      <MonacoEditor
        value={content.query.data}
        language={currentProfile.data?.language}
        path={currentProfile.data?.virtualPath}
        theme={themeMode === 'light' ? 'vs' : 'vs-dark'}
        // TODO: implement this
        // beforeMount={beforeEditorMount}
        // onMount={handleEditorDidMount}
        onChange={setEditorValue}
        // onValidate={onValidate}
        loading={<LoadingSkeleton />}
        options={{
          readOnly: readonly,
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

      <div className="bg-background flex h-14 items-center gap-2 px-3">
        <ActionButton>{m.common_validate()}</ActionButton>
        <ActionButton onClick={handleReset}>{m.common_reset()}</ActionButton>

        <div className="flex-1" />

        <ActionButton onClick={handleCancel}>{m.common_cancel()}</ActionButton>

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
  )
}
