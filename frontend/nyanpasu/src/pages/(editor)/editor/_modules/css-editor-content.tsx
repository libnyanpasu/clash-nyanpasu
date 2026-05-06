import { ComponentProps, useEffect, useState } from 'react'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import useCustomCss from '@/hooks/use-custom-css'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { registerCssDataSlotCompletion } from '@/utils/monaco-css'
import MonacoEditor from '@monaco-editor/react'
import { useKvStorage } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { message } from '@tauri-apps/plugin-dialog'
import Header from './header'
import { MONACO_FONT_FAMILY } from './utils'

const currentWindow = getCurrentWebviewWindow()

const ActionButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => {
  return <Button className={cn('h-8 min-w-0 px-3', className)} {...props} />
}

export default function CssEditorContent() {
  const { themeMode } = useExperimentalThemeContext()

  const customCss = useCustomCss()
  const [editorValue, setEditorValue] = useState('')
  const [initialized, setInitialized] = useState(false)

  // Sync the initial value once the KV storage has loaded
  useEffect(() => {
    if (!initialized && !customCss.isLoading) {
      setEditorValue(customCss.value ?? '')
      setInitialized(true)
    }
  }, [customCss.value, customCss.isLoading, initialized])

  const handleSave = useLockFn(async () => {
    try {
      await customCss.upsert(editorValue)
      await currentWindow.close()
    } catch {
      await message(m.custom_css_save_error(), { kind: 'error' })
    }
  })

  const handleClear = useLockFn(async () => {
    await customCss.upsert('')
    setEditorValue('')
  })

  const handleCancel = useLockFn(async () => {
    await currentWindow.close()
  })

  return (
    <>
      <Header title="Clash Nyanpasu - CSS Editor" />

      <div className="min-h-0 flex-1" data-slot="editor-content">
        <MonacoEditor
          className="h-full w-full"
          language="css"
          value={editorValue}
          theme={themeMode === 'light' ? 'vs' : 'vs-dark'}
          beforeMount={(monacoInstance) =>
            registerCssDataSlotCompletion(monacoInstance)
          }
          onChange={(v) => setEditorValue(v ?? '')}
          options={{
            minimap: { enabled: false },
            wordWrap: 'on',
            automaticLayout: true,
            fontFamily: MONACO_FONT_FAMILY,
            quickSuggestions: { strings: true, comments: false, other: true },
          }}
        />
      </div>

      <div
        className="bg-background flex h-12 shrink-0 items-center gap-2 px-3"
        data-slot="editor-footer-actions"
      >
        <ActionButton onClick={handleClear}>{m.common_clear()}</ActionButton>

        <div className="flex-1" />

        <ActionButton onClick={handleCancel}>{m.common_cancel()}</ActionButton>

        <ActionButton className="px-5" variant="flat" onClick={handleSave}>
          {m.common_save()}
        </ActionButton>
      </div>
    </>
  )
}
