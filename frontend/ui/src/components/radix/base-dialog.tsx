import { AnimatePresence, motion } from 'framer-motion'
import React, {
  CSSProperties,
  ReactNode,
  useEffect,
  useLayoutEffect,
  useState,
} from 'react'
import { getSystem } from '@/hooks'
import { cn } from '../../utils/cn'
import { Button } from './button'
import { DialogPortal } from './dialog'

const OS = getSystem()

export interface RadixBaseDialogProps {
  title: ReactNode
  open: boolean
  close?: string
  ok?: string
  disabledOk?: boolean
  contentStyle?: CSSProperties
  children?: ReactNode
  loading?: boolean
  full?: boolean
  onOk?: () => void | Promise<void>
  onClose?: () => void | Promise<void>
  divider?: boolean
}

export const RadixBaseDialog: React.FC<RadixBaseDialogProps> = ({
  title,
  open,
  close,
  onClose,
  children,
  contentStyle,
  disabledOk,
  loading,
  full,
  onOk,
  ok,
  divider,
}) => {
  const [mounted, setMounted] = useState(false)
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const [okLoading, setOkLoading] = useState(false)
  const [closeLoading, setCloseLoading] = useState(false)

  useLayoutEffect(() => {
    if (open) {
      // fall back to center grow if click position hook isn't available
      setOffset({ x: window.innerWidth / 2, y: window.innerHeight / 2 })
    }
  }, [open])

  const handleClose = async () => {
    if (!onClose) return
    try {
      setCloseLoading(true)
      await onClose()
    } finally {
      setCloseLoading(false)
      setMounted(false)
    }
  }

  const handleOk = async () => {
    if (!onOk) return
    try {
      setOkLoading(true)
      await onOk()
    } finally {
      setOkLoading(false)
    }
  }

  useEffect(() => {
    if (open) {
      setMounted(true)
    } else {
      // trigger exit animation then unmount via AnimatePresence
      setMounted(false)
    }
  }, [open])

  return (
    <AnimatePresence initial={false}>
      {open && (
        <DialogPortal>
          {!full && (
            <motion.div
              className={cn(
                'fixed inset-0 z-50',
                OS === 'linux' ? 'bg-black/50' : 'backdrop-blur-xl',
              )}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              onClick={handleClose}
            />
          )}

          <motion.div
            className={cn(
              'text-on-surface fixed top-1/2 left-1/2 z-50',
              full ? 'h-dvh w-full' : 'min-w-96 rounded-3xl shadow',
            )}
            style={{
              backgroundColor: 'var(--md3-color-surface-container)',
              translateX: '-50%',
              translateY: '-50%',
            }}
            initial={{
              opacity: 0.3,
              scale: 0,
              x: offset.x - window.innerWidth / 2,
              y: offset.y - window.innerHeight / 2,
            }}
            animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
            exit={{
              opacity: 0.3,
              scale: 0,
              x: offset.x - window.innerWidth / 2,
              y: offset.y - window.innerHeight / 2,
            }}
            transition={{ type: 'spring', bounce: 0, duration: 0.35 }}
          >
            <div
              className={cn(
                'text-xl',
                !full ? 'm-4' : OS === 'macos' ? 'ml-20 p-3.5' : 'm-2 ml-6',
              )}
              data-tauri-drag-region={full}
            >
              {title}
            </div>

            {divider && <div className="bg-outline-variant/50 h-px w-full" />}

            <div
              className={cn(
                'relative overflow-x-hidden overflow-y-auto p-4',
                full && 'h-full px-6',
              )}
              style={{
                maxHeight: full
                  ? `calc(100vh - ${OS === 'macos' ? 114 : 100}px)`
                  : 'calc(100vh - 200px)',
                ...contentStyle,
              }}
            >
              {children}
            </div>

            {divider && <div className="bg-outline-variant/50 h-px w-full" />}

            <div className={cn('m-2 flex justify-end gap-2', full && 'mx-6')}>
              {onClose && (
                <Button
                  disabled={loading || closeLoading}
                  onClick={handleClose}
                  variant="outlined"
                >
                  {close || 'Close'}
                </Button>
              )}

              {onOk && (
                <Button
                  disabled={loading || disabledOk}
                  onClick={handleOk}
                  variant="filled"
                >
                  {ok || 'Ok'}
                </Button>
              )}
            </div>
          </motion.div>
        </DialogPortal>
      )}
    </AnimatePresence>
  )
}

export default RadixBaseDialog
