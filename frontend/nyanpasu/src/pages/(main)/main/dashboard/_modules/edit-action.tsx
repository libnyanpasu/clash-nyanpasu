import AddRounded from '~icons/material-symbols/add-rounded'
import DoneRounded from '~icons/material-symbols/done-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { useDashboardContext } from './provider'

export default function EditAction() {
  const { isEditing, setIsEditing, setOpenSheet } = useDashboardContext()

  return (
    <AnimatePresence>
      {isEditing && (
        <motion.div
          className={cn(
            'absolute bottom-8 left-1/2 z-10 -translate-x-1/2',
            'flex h-14 min-w-0 items-center justify-between gap-6 px-3',
            'dark:border-surface-variant/50 border-surface/50 rounded-full border',
            'dark:bg-surface/30 bg-surface-variant/30 backdrop-blur-lg',
          )}
          data-slot="dashboard-edit-header"
          initial={{
            scale: 0.8,
            opacity: 0,
            y: 128,
          }}
          animate={{
            scale: 1,
            opacity: 1,
            y: 0,
          }}
          exit={{
            scale: 0.8,
            opacity: 0,
            y: 128,
          }}
          transition={{
            type: 'spring',
            bounce: 0,
            duration: 0.45,
          }}
        >
          <Button
            variant="raised"
            className="flex h-8 items-center gap-1 px-3 text-sm text-nowrap"
            onClick={() => setOpenSheet(true)}
          >
            <AddRounded />

            <span>{m.dashboard_add_widget()}</span>
          </Button>

          <Button
            variant="flat"
            className="flex h-8 items-center gap-1 px-3 text-sm text-nowrap"
            onClick={() => setIsEditing(false)}
          >
            <DoneRounded />

            <span>{m.common_save()}</span>
          </Button>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
