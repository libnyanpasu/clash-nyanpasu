import { motion } from 'framer-motion'
import { merge } from 'lodash-es'
import {
  createContext,
  use,
  type ComponentProps,
  type PropsWithChildren,
} from 'react'
import { cn } from '@nyanpasu/ui'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

const DEFAULT_SIDEBAR_WIDTH = {
  open: 280,
  closed: 48 + 8 * 2,
}

const SidebarContext = createContext<{
  open: boolean
  setOpen: (isOpen: boolean) => void
} | null>(null)

export const useSidebar = () => {
  const context = use(SidebarContext)

  if (!context) {
    throw new Error('useSidebar must be used within a SidebarProvider')
  }

  return context
}

export function SidebarProvider({
  open: inputOpen,
  onOpenChange,
  defaultOpen,
  children,
}: PropsWithChildren & {
  open?: boolean
  onOpenChange?: (isOpen: boolean) => void
  defaultOpen?: boolean
}) {
  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <SidebarContext.Provider
      value={{
        open,
        setOpen,
      }}
    >
      {children}
    </SidebarContext.Provider>
  )
}

export function Sidebar({
  animate,
  transition,
  width = DEFAULT_SIDEBAR_WIDTH,
  ...props
}: ComponentProps<typeof motion.aside> & {
  width?: {
    open?: number
    closed?: number
  }
}) {
  const { open } = useSidebar()

  return (
    <motion.aside
      initial={false}
      animate={merge(
        {
          width: open ? width.open : width.closed,
        },
        animate,
      )}
      transition={{
        type: 'spring',
        stiffness: 300,
        damping: 30,
        ...transition,
      }}
      {...props}
    />
  )
}

export function SidebarLabelItem({
  className,
  animate,
  transition,
  ...props
}: ComponentProps<typeof motion.span>) {
  const { open } = useSidebar()

  return (
    <motion.span
      className={cn('overflow-hidden whitespace-nowrap', className)}
      initial={false}
      animate={merge(
        {
          width: open ? '100%' : 0,
        },
        animate,
      )}
      transition={{
        duration: 0.2,
        ease: 'easeOut',
        ...transition,
      }}
      {...props}
    />
  )
}
