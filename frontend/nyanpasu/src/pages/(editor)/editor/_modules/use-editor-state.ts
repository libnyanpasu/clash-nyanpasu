import type { editor } from 'monaco-editor'
import { useEffect, useState } from 'react'

const INITIAL_STATE = { line: 1, column: 1, canUndo: false, canRedo: false }

/** The cursor position and history state of `instance`, as they change. */
export function useEditorState(instance: editor.IStandaloneCodeEditor | null) {
  const [state, setState] = useState(INITIAL_STATE)

  useEffect(() => {
    if (!instance) return

    const update = () => {
      const position = instance.getPosition()
      const model = instance.getModel()

      setState({
        line: position?.lineNumber ?? 1,
        column: position?.column ?? 1,
        canUndo: model?.canUndo() ?? false,
        canRedo: model?.canRedo() ?? false,
      })
    }

    update()

    const subscriptions = [
      instance.onDidChangeCursorPosition(update),
      instance.onDidChangeModelContent(update),
      instance.onDidChangeModel(update),
    ]

    return () => subscriptions.forEach((subscription) => subscription.dispose())
  }, [instance])

  return state
}
