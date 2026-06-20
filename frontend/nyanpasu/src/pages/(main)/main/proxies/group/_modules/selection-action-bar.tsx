import AddRounded from '~icons/material-symbols/add-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import DoneAllRounded from '~icons/material-symbols/done-all-rounded'
import RemoveDoneRounded from '~icons/material-symbols/remove-done-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { useGroupSelection } from './selection'

export default function SelectionActionBar({
  nodeNames,
}: {
  nodeNames: string[]
}) {
  const { selecting, count, selectAll, clear, openCreate, exit } =
    useGroupSelection()

  const allSelected = count > 0 && count === nodeNames.length

  return (
    <AnimatePresence>
      {selecting && (
        <motion.div
          data-slot="selection-action-bar"
          className={cn(
            'absolute bottom-4 left-1/2 z-20 w-fit -translate-x-1/2',
            'flex items-center gap-1 rounded-2xl p-2',
            'backdrop-blur',
            'bg-primary-container/40 dark:bg-on-primary/35',
            'shadow-inverse-surface/30 shadow-lg',
          )}
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 20 }}
          transition={{ type: 'spring', bounce: 0, duration: 0.35 }}
        >
          <span className="px-2 text-sm font-medium whitespace-nowrap">
            {m.proxies_group_create_group_selected_count({
              count: String(count),
            })}
          </span>

          <Button
            icon
            className="size-9"
            onClick={() => (allSelected ? clear() : selectAll(nodeNames))}
          >
            {allSelected ? (
              <RemoveDoneRounded className="size-5" />
            ) : (
              <DoneAllRounded className="size-5" />
            )}
          </Button>

          <Button
            variant="raised"
            className="flex items-center gap-1 px-3"
            disabled={count === 0}
            onClick={openCreate}
          >
            <AddRounded className="size-5" />
            <span>{m.proxies_group_create_group_action()}</span>
          </Button>

          <Button icon className="size-9" onClick={exit}>
            <CloseRounded className="size-5" />
          </Button>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
