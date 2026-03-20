import AddRounded from '~icons/material-symbols/add-rounded'
import DoneRounded from '~icons/material-symbols/done-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'
import { useDashboardContext } from './provider'

export default function HeaderAction() {
  const { isEditing, setIsEditing, setOpenSheet } = useDashboardContext()

  return (
    <AnimatePresence>
      {isEditing && (
        <motion.div
          className="flex w-full items-center justify-between"
          data-slot="dashboard-action-header"
          initial={{
            opacity: 0,
            height: 0,
            y: -10,
          }}
          animate={{
            opacity: 1,
            height: 'auto',
            y: 0,
          }}
          exit={{
            opacity: 0,
            height: 0,
            y: -10,
          }}
          transition={{
            type: 'spring',
            bounce: 0,
            duration: 0.3,
          }}
        >
          <Button
            variant="raised"
            className="flex h-8 items-center gap-1 px-3 text-sm"
            onClick={() => setOpenSheet(true)}
          >
            <AddRounded />

            <span>{m.dashboard_add_widget()}</span>
          </Button>

          <div className="flex-1" />

          <Button
            variant="raised"
            className="flex h-8 items-center gap-1 px-3 text-sm"
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
