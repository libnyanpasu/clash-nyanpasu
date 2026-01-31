import CloseRounded from '~icons/material-symbols/close-rounded'
import Crop54Outline from '~icons/material-symbols/crop-5-4-outline'
import FilterNoneRounded from '~icons/material-symbols/filter-none-outline-rounded'
import HorizontalRuleRounded from '~icons/material-symbols/horizontal-rule-rounded'
import PushPin from '~icons/material-symbols/push-pin'
import PushPinOutline from '~icons/material-symbols/push-pin-outline'
import { AnimatePresence, motion } from 'framer-motion'
import { ComponentProps, useCallback } from 'react'
import { Button, ButtonProps } from '@/components/ui/button'
import useWindowMaximized from '@/hooks/use-window-maximized'
import { useSetting } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

const appWindow = getCurrentWebviewWindow()

const CtrlButton = ({ className, ...props }: ButtonProps) => {
  return (
    <Button
      className={cn(
        'hover:bg-primary-container dark:hover:bg-on-primary size-8',
        className,
      )}
      icon
      {...props}
    />
  )
}

const AlwaysOnTopButton = () => {
  const { value: alwaysOnTop, upsert: upsertAlwaysOnTop } =
    useSetting('always_on_top')

  const handleToggleAlwaysOnTop = useCallback(async () => {
    await upsertAlwaysOnTop(!alwaysOnTop)
    await appWindow.setAlwaysOnTop(!alwaysOnTop)
  }, [alwaysOnTop, upsertAlwaysOnTop])

  return (
    <CtrlButton
      onClick={handleToggleAlwaysOnTop}
      data-slot="window-control-always-on-top-button"
    >
      <AnimatePresence mode="wait">
        <motion.span
          key={alwaysOnTop ? 'pinned' : 'unpinned'}
          className="flex items-center justify-center"
          initial={{ opacity: 0, scale: 0.7 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0, rotate: 35, scale: 0.8 }}
          transition={{ duration: 0.2 }}
        >
          {alwaysOnTop ? (
            <PushPin className="size-5 rotate-15" />
          ) : (
            <PushPinOutline className="size-5 rotate-15" />
          )}
        </motion.span>
      </AnimatePresence>
    </CtrlButton>
  )
}

const MinimizeButton = () => {
  const handleMinimize = useCallback(async () => {
    await appWindow.minimize()
  }, [])

  return (
    <CtrlButton
      onClick={handleMinimize}
      data-slot="window-control-minimize-button"
    >
      <HorizontalRuleRounded className="size-5" />
    </CtrlButton>
  )
}

const MaximizeButton = () => {
  const { isMaximized, toggleMaximize } = useWindowMaximized()

  return (
    <CtrlButton
      onClick={toggleMaximize}
      data-slot="window-control-maximize-button"
    >
      {isMaximized ? (
        <FilterNoneRounded className="size-4.5 rotate-180" />
      ) : (
        <Crop54Outline className="size-4.5" />
      )}
    </CtrlButton>
  )
}

const CloseButton = () => {
  const handleClose = useCallback(async () => {
    await appWindow.close()
  }, [])

  return (
    <CtrlButton onClick={handleClose} data-slot="window-control-close-button">
      <CloseRounded className="size-5.5" />
    </CtrlButton>
  )
}

export default function WindowControl({ className }: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex gap-1', className)}
      data-slot="window-control"
      data-tauri-drag-region
    >
      <AlwaysOnTopButton />

      <MinimizeButton />

      <MaximizeButton />

      <CloseButton />
    </div>
  )
}
