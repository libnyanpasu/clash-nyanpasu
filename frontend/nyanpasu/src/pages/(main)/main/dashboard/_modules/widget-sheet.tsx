import CloseRounded from '~icons/material-symbols/close-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/ui'

export function WidgetSheet({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className={cn(
            'absolute right-0 bottom-0 left-0 h-96 rounded-t-2xl p-4',
            'bg-on-primary-container/5 backdrop-blur-3xl',
          )}
          initial={{ y: '100%' }}
          animate={{ y: 0 }}
          exit={{ y: '100%' }}
          transition={{
            type: 'spring',
            bounce: 0,
            duration: 0.35,
          }}
        >
          <div className="flex w-full items-center justify-between gap-4">
            <div>1</div>

            <Button icon onClick={() => onOpenChange(false)}>
              <CloseRounded />
            </Button>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
