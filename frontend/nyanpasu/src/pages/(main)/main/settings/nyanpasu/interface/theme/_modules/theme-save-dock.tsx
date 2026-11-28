import CheckRounded from '~icons/material-symbols/check-rounded'
import UndoRounded from '~icons/material-symbols/undo-rounded'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { ActionSwapText } from '@nyanpasu/ui/action-swap-text'
import { Button } from '@nyanpasu/ui/button'
import { GlassControlGroup } from '@nyanpasu/ui/glass-control-group'
import { m } from '@/paraglide/messages'

/** How long the dock confirms a saved theme before it leaves. */
const APPLIED_NOTICE_MS = 2000

/**
 * The unsaved-changes dock in the window's bottom-right corner: it rises
 * with the first edit, and after a save confirms it briefly before leaving.
 * Rendered into `body`, since the route transition transforms the page and
 * would pin a `fixed` box to the page instead of the window.
 */
export function ThemeSaveDock({
  dirty,
  applying,
  onDiscard,
  onApply,
}: {
  dirty: boolean
  applying: boolean
  onDiscard: () => void
  onApply: () => Promise<boolean>
}) {
  const reduceMotion = useReducedMotion()
  const [applied, setApplied] = useState(false)

  useEffect(() => {
    if (!applied) {
      return
    }

    const timer = setTimeout(() => setApplied(false), APPLIED_NOTICE_MS)

    return () => clearTimeout(timer)
  }, [applied])

  const status = applying
    ? m.settings_user_interface_theme_applying()
    : dirty
      ? m.settings_user_interface_theme_unsaved()
      : applied
        ? m.settings_user_interface_theme_applied()
        : null

  const handleApply = async () => {
    setApplied(await onApply())
  }

  const handleDiscard = () => {
    setApplied(false)
    onDiscard()
  }

  return createPortal(
    <AnimatePresence>
      {status !== null && (
        <motion.div
          data-slot="theme-save-dock"
          className="fixed right-4 bottom-4 z-40"
          initial={{ opacity: 0, y: reduceMotion ? 0 : 16 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: reduceMotion ? 0 : 16 }}
          transition={{ duration: reduceMotion ? 0 : 0.2, ease: 'easeOut' }}
        >
          <GlassControlGroup role="status" className="gap-1.5 pl-4">
            <ActionSwapText
              className="text-on-surface text-sm font-medium whitespace-nowrap"
              value={status}
            />

            <Button
              data-slot="theme-save-dock-discard"
              variant="glass"
              className="gap-1.5"
              disabled={!dirty || applying}
              onClick={handleDiscard}
            >
              <UndoRounded className="size-4" />
              {m.settings_user_interface_theme_discard()}
            </Button>

            <Button
              data-slot="theme-save-dock-apply"
              variant="flat"
              className="gap-1.5"
              disabled={!dirty || applying}
              loading={applying}
              onClick={handleApply}
            >
              <CheckRounded className="size-4" />
              {m.common_apply()}
            </Button>
          </GlassControlGroup>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  )
}
