import '@/services/monaco'
import { DiffEditor, DiffEditorProps } from '@monaco-editor/react'
import { beforeEditorMount } from './profile-monaco-viewer'

export default function ProfileMonacoDiffViewer(
  props: Omit<DiffEditorProps, 'beforeMount'>,
) {
  return <DiffEditor {...props} beforeMount={beforeEditorMount} />
}
