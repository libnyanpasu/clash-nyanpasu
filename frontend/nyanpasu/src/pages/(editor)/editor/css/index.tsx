import type { editor } from 'monaco-editor'
import { useEffect, useState } from 'react'
import z from 'zod'
import useCustomCss from '@/hooks/use-custom-css'
import { m } from '@/paraglide/messages'
import { registerCssDataSlotCompletion } from '@/utils/monaco-css'
import { ask, message } from '@/utils/notification'
import MonacoEditor from '@monaco-editor/react'
import { useLockFn } from '@nyanpasu/hooks'
import { createWindowAdapter } from '@nyanpasu/platform'
import { usePlatform } from '@nyanpasu/platform/react'
import { createFileRoute } from '@tanstack/react-router'
import ActionButton from '../_modules/action-button'
import { EditorFooter } from '../_modules/footer'
import Header from '../_modules/header'
import { EDITOR_SHORTCUTS, useShortcuts } from '../_modules/shortcuts'
import { EditorStatus } from '../_modules/status'
import {
  EditorToolbar,
  EditorTools,
  ShortcutTooltip,
} from '../_modules/toolbar'
import { useMonacoTheme } from '../_modules/use-monaco-theme'
import { getMonacoFontFamily } from '../_modules/utils'

const EMPTY_CSS_TEMPLATE = `/* Welcome to Clash Nyanpasu CSS/LESS Editor! */
/* You can write standard CSS or LESS syntax to customize the appearance of the app. */
/* For example, you can change the background color of the header: */

/*
@primary: #ff69b4;

#app-header {
  background-color: @primary;
}
*/

/* Feel free to experiment and make the app your own! */
/* Show more at https://nyanpasu.org/tutorial/custom-css/ */
`

export const Route = createFileRoute('/(editor)/editor/css/')({
  component: RouteComponent,
  validateSearch: z.object({
    uid: z.string(),
  }),
})

function RouteComponent() {
  const { uid } = Route.useSearch()

  const { clientOs } = usePlatform()
  const [currentWindow] = useState(createWindowAdapter)
  const monacoTheme = useMonacoTheme()

  const [editorInstance, setEditorInstance] =
    useState<editor.IStandaloneCodeEditor | null>(null)
  const [markers, setMarkers] = useState<editor.IMarker[]>([])
  const [wordWrap, setWordWrap] = useState(true)

  const customCss = useCustomCss()
  const item = customCss.state.items.find((item) => item.id === uid)
  const [editorValue, setEditorValue] = useState('')
  // What the editor showed when it last matched the stored source.
  const [savedValue, setSavedValue] = useState('')
  const [initialized, setInitialized] = useState(false)
  const isDirty = initialized && editorValue !== savedValue

  // Sync the initial value once the KV storage has loaded
  useEffect(() => {
    if (!initialized && !customCss.isLoading && item) {
      setEditorValue(item.source || EMPTY_CSS_TEMPLATE)
      setSavedValue(item.source || EMPTY_CSS_TEMPLATE)
      setInitialized(true)
    }
  }, [item, customCss.isLoading, initialized])

  const handleSave = useLockFn(async (close?: boolean) => {
    try {
      await customCss.saveSource(uid, editorValue)
      setSavedValue(editorValue)
      if (close) {
        await currentWindow?.close()
      }
    } catch (error) {
      await message(m.custom_css_save_error(), { kind: 'error', error })
    }
  })

  const handleClear = useLockFn(async () => {
    await customCss.saveSource(uid, '')
    setEditorValue('')
    setSavedValue('')
  })

  const handleBeforeClose = useLockFn(
    async () =>
      !isDirty ||
      (await ask(m.editor_before_close_message(), { kind: 'warning' })),
  )

  const handleCancel = useLockFn(async () => {
    if (await handleBeforeClose()) {
      await currentWindow?.close()
    }
  })

  useShortcuts([
    [EDITOR_SHORTCUTS.apply, () => handleSave(false)],
    [EDITOR_SHORTCUTS.save, () => handleSave(true)],
    [EDITOR_SHORTCUTS.close, handleCancel],
    [EDITOR_SHORTCUTS.wordWrap, () => setWordWrap((wordWrap) => !wordWrap)],
  ])

  return (
    <>
      <Header
        title={item && `${item.name}.less`}
        dirty={isDirty}
        beforeClose={handleBeforeClose}
      />

      <EditorToolbar>
        <div className="text-sm font-medium" data-slot="editor-header-language">
          LESS
        </div>

        <div className="flex-1" />

        <EditorTools
          editor={editorInstance}
          wordWrap={wordWrap}
          onWordWrapChange={setWordWrap}
        />
      </EditorToolbar>

      <div className="min-h-0 flex-1" data-slot="editor-content">
        <MonacoEditor
          className="h-full w-full"
          language="less"
          value={editorValue}
          theme={monacoTheme}
          beforeMount={(monacoInstance) =>
            registerCssDataSlotCompletion(monacoInstance)
          }
          onMount={setEditorInstance}
          onChange={(v) => setEditorValue(v ?? '')}
          onValidate={setMarkers}
          options={{
            minimap: { enabled: false },
            wordWrap: wordWrap ? 'on' : 'off',
            automaticLayout: true,
            fontFamily: getMonacoFontFamily(clientOs),
            quickSuggestions: { strings: true, comments: false, other: true },
          }}
        />
      </div>

      <EditorFooter>
        <EditorStatus editor={editorInstance} markers={markers} />

        <div className="flex-1" />

        <ActionButton onClick={handleClear}>{m.common_clear()}</ActionButton>

        <ShortcutTooltip shortcut={EDITOR_SHORTCUTS.close}>
          <ActionButton onClick={handleCancel}>
            {m.common_cancel()}
          </ActionButton>
        </ShortcutTooltip>

        <ShortcutTooltip shortcut={EDITOR_SHORTCUTS.apply}>
          <ActionButton
            className="px-5"
            variant="flat"
            onClick={() => handleSave(false)}
          >
            {m.common_apply()}
          </ActionButton>
        </ShortcutTooltip>

        <ShortcutTooltip shortcut={EDITOR_SHORTCUTS.save}>
          <ActionButton
            className="px-5"
            variant="flat"
            onClick={() => handleSave(true)}
          >
            {m.common_save()}
          </ActionButton>
        </ShortcutTooltip>
      </EditorFooter>
    </>
  )
}
