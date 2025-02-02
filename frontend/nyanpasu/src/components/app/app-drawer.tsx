import { AnimatePresence, motion } from 'framer-motion'
import { useState } from 'react'
import getSystem from '@/utils/get-system'
import { MenuOpen } from '@mui/icons-material'
import { alpha, Backdrop, IconButton } from '@mui/material'
import { cn } from '@nyanpasu/ui'
import AnimatedLogo from '../layout/animated-logo'
import DrawerContent from './drawer-content'

const OS = getSystem()

export const AppDrawer = () => {
  const [open, setOpen] = useState(false)

  const DrawerTitle = () => {
    return (
      <div
        className={cn(
          'fixed z-10 flex items-center gap-2',
          OS === 'macos' ? 'top-3 left-24' : 'top-1.5 left-4',
        )}
        data-tauri-drag-region
      >
        <IconButton
          className="!size-8 !min-w-0"
          sx={[
            (theme) => ({
              backgroundColor: alpha(theme.palette.primary.main, 0.1),
              svg: { transform: 'scale(0.9)' },
            }),
          ]}
          onClick={() => setOpen(true)}
        >
          <MenuOpen />
        </IconButton>

        <div className="size-5" data-tauri-drag-region>
          <AnimatedLogo className="h-full w-full" data-tauri-drag-region />
        </div>

        <div className="text-lg" data-tauri-drag-region>
          Clash Nyanpasu
        </div>
      </div>
    )
  }

  return (
    <>
      <DrawerTitle />
      <Backdrop
        className={cn('z-20', OS !== 'linux' && 'backdrop-blur-xl')}
        sx={[
          (theme) =>
            OS === 'linux'
              ? {
                  backgroundColor: null,
                }
              : {
                  backgroundColor: alpha(theme.palette.primary.light, 0.1),
                  ...theme.applyStyles('dark', {
                    backgroundColor: alpha(theme.palette.primary.dark, 0.1),
                  }),
                },
        ]}
        open={open}
        onClick={() => setOpen(false)}
      >
        <AnimatePresence initial={false}>
          <div className="h-full w-full">
            <motion.div
              className="h-full"
              animate={open ? 'open' : 'closed'}
              variants={{
                open: {
                  x: 0,
                },
                closed: {
                  x: -240,
                },
              }}
              transition={{
                type: 'tween',
              }}
            >
              <DrawerContent className="max-w-64" />
            </motion.div>
          </div>
        </AnimatePresence>
      </Backdrop>
    </>
  )
}

export default AppDrawer
