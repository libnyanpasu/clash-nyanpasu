import { nanoid } from 'nanoid'
import { ComponentProps, useEffect, useMemo, useState } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import { CircularProgress } from '@/components/ui/progress'
import { OS } from '@/consts'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import MonacoEditor from '@monaco-editor/react'
import { useProfile, useProfileContent } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { Route as EditorRoute } from './route'

const currentWindow = getCurrentWebviewWindow()

const MONACO_FONT_FAMILY =
  '"Cascadia Code NF",' +
  '"Cascadia Code",' +
  'Fira Code,' +
  'JetBrains Mono,' +
  'Roboto Mono,' +
  '"Source Code Pro",' +
  'Consolas,' +
  'Menlo,' +
  'Monaco,' +
  'monospace,' +
  `${OS === 'windows' ? 'twemoji mozilla' : ''}`

export const Route = createFileRoute('/(editor)/editor/')({
  component: RouteComponent,
})

const LoadingSkeleton = () => {
  return (
    <div className="grid flex-1 place-items-center">
      <CircularProgress className="size-12" indeterminate />
    </div>
  )
}

const ActionButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => {
  return <Button className={cn('h-8 min-w-0 px-3', className)} {...props} />
}

function RouteComponent() {
  const { themeMode } = useExperimentalThemeContext()

  const { uid, readonly } = EditorRoute.useSearch()

  const profiles = useProfile()

  const currentProfile = useMemo(() => {
    const item = profiles.query.data?.items?.find((item) => item.uid === uid)

    if (item) {
      let language = 'yaml'

      if (item.type === 'script') {
        if (item.script_type === 'javascript') {
          language = 'javascript'
        }

        if (item.script_type === 'lua') {
          language = 'lua'
        }
      }

      return {
        ...item,
        language,
        virtualPath: `${nanoid()}.${language}`,
      }
    }
  }, [profiles.query.data, uid])

  const content = useProfileContent(uid)

  const [editorValue, setEditorValue] = useState<string>()

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

  const handleCancel = useLockFn(currentWindow.close)

  const handleReset = useLockFn(async () => {
    setEditorValue(content.query.data)
  })

  // loading state
  if (content.query.isLoading || profiles.query.isLoading) {
    return <LoadingSkeleton />
  }

  return (
    <>
      <div
        className={cn(
          'dark:bg-on-primary bg-primary-container flex items-center px-3',
          'h-12',
        )}
        data-slot="editor-header-actions"
      >
        <div className="text-sm font-medium" data-slot="editor-header-title">
          {currentProfile?.name}.{currentProfile?.language}
        </div>
      </div>

      <MonacoEditor
        className="flex-1"
        value={content.query.data}
        language={currentProfile?.language}
        path={currentProfile?.virtualPath}
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
          tabSize: currentProfile?.language === 'yaml' ? 2 : 4,
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
