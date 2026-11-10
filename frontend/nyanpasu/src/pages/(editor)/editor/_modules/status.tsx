import CheckCircleOutlineRounded from '~icons/material-symbols/check-circle-outline-rounded'
import ErrorOutlineRounded from '~icons/material-symbols/error-outline-rounded'
import WarningOutlineRounded from '~icons/material-symbols/warning-outline-rounded'
import { MarkerSeverity, type editor } from 'monaco-editor'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import ActionButton from './action-button'
import { useEditorState } from './use-editor-state'

/** Whether any of `markers` must be fixed before the content is saved. */
export const hasErrors = (markers: readonly editor.IMarker[]) =>
  markers.some((marker) => marker.severity === MarkerSeverity.Error)

/** The problems Monaco found, which a click steps through, and the cursor. */
export function EditorStatus({
  editor: instance,
  markers,
}: {
  editor: editor.IStandaloneCodeEditor | null
  markers: readonly editor.IMarker[]
}) {
  const { line, column } = useEditorState(instance)

  const errors = markers.filter(
    (marker) => marker.severity === MarkerSeverity.Error,
  ).length
  const warnings = markers.filter(
    (marker) => marker.severity === MarkerSeverity.Warning,
  ).length
  const hasProblems = errors + warnings > 0

  const handleNextProblem = () => {
    instance?.focus()
    instance?.trigger('status', 'editor.action.marker.next', null)
  }

  return (
    <div
      className="text-on-surface-variant flex min-w-0 items-center gap-2 text-xs"
      data-slot="editor-status"
    >
      <ActionButton
        className={cn(
          'flex items-center gap-1.5 px-2 text-xs',
          errors > 0 && 'text-error dark:text-error',
        )}
        disabled={!hasProblems}
        aria-label={
          hasProblems
            ? m.editor_status_problems({ errors, warnings })
            : m.editor_status_no_problems()
        }
        onClick={handleNextProblem}
        data-slot="editor-status-problems"
      >
        {hasProblems ? (
          <>
            <ErrorOutlineRounded className="size-4" />
            {errors}
            <WarningOutlineRounded className="size-4" />
            {warnings}
          </>
        ) : (
          <CheckCircleOutlineRounded className="size-4" />
        )}
      </ActionButton>

      <span
        className="font-mono text-nowrap tabular-nums"
        data-slot="editor-status-position"
      >
        {m.editor_status_position({ line, column })}
      </span>
    </div>
  )
}
