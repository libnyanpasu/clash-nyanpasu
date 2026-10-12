import Close from '~icons/material-symbols/close-rounded'
import { AnimatePresence, motion, type Variants } from 'motion/react'
import { DropdownMenu as DropdownMenuPrimitive } from 'radix-ui'
import {
  createContext,
  useContext,
  type ComponentProps,
  type ReactNode,
} from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { Button, type ButtonProps } from './button'

/**
 * Material 3 FAB menu: a FAB that expands into a vertical stack of labeled
 * actions. The FAB turns into a round close button while the menu is open.
 * Focus, keyboard navigation and dismissal come from the Radix dropdown menu.
 *
 * @see https://m3.material.io/components/fab-menu/overview
 */

const FabMenuContext = createContext<{ open: boolean } | null>(null)

const useFabMenu = () => {
  const context = useContext(FabMenuContext)

  if (context === null) {
    throw new Error('FabMenu components must be used within <FabMenu>')
  }

  return context
}

const contentVariants = {
  closed: { transition: { staggerChildren: 0.03, staggerDirection: -1 } },
  open: { transition: { staggerChildren: 0.04, staggerDirection: -1 } },
} satisfies Variants

const itemVariants = {
  closed: { opacity: 0, y: 16, scale: 0.8 },
  open: {
    opacity: 1,
    y: 0,
    scale: 1,
    transition: { type: 'spring', bounce: 0.2, duration: 0.4 },
  },
} satisfies Variants

export function FabMenu({
  open: inputOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Root>) {
  const [open, setOpen] = useControllableState({
    prop: inputOpen,
    defaultProp: defaultOpen ?? false,
    onChange: onOpenChange,
  })

  return (
    <FabMenuContext.Provider value={{ open }}>
      <DropdownMenuPrimitive.Root
        modal={false}
        {...props}
        open={open}
        onOpenChange={setOpen}
      />
    </FabMenuContext.Provider>
  )
}

export interface FabMenuTriggerProps extends Omit<
  ButtonProps,
  'variant' | 'icon' | 'children'
> {
  /** Icon of the collapsed FAB; the expanded state always shows a close icon. */
  icon: ReactNode
}

export function FabMenuTrigger({
  icon,
  className,
  ...props
}: FabMenuTriggerProps) {
  const { open } = useFabMenu()

  return (
    <DropdownMenuPrimitive.Trigger asChild>
      <Button
        variant="fab"
        icon
        data-slot="fab-menu-trigger"
        className={cn(
          'transition-[background-color,color,shadow,filter,border-radius]',
          'duration-300 ease-[cubic-bezier(0.2,0,0,1)]',
          // Animate to half the 56px FAB height; `rounded-full` (9999px) would
          // finish its visible change within the first frames.
          open && [
            'rounded-[28px]',
            'bg-primary text-on-primary dark:bg-primary dark:text-on-primary',
          ],
          className,
        )}
        {...props}
      >
        <AnimatePresence initial={false} mode="wait">
          <motion.span
            key={open ? 'close' : 'open'}
            className="grid size-6 place-content-center"
            initial={{ opacity: 0, rotate: -90 }}
            animate={{ opacity: 1, rotate: 0 }}
            exit={{ opacity: 0, rotate: 90 }}
            transition={{ duration: 0.12 }}
          >
            {open ? <Close className="size-6" /> : icon}
          </motion.span>
        </AnimatePresence>
      </Button>
    </DropdownMenuPrimitive.Trigger>
  )
}

/** Opens upward from the trigger, aligned to its trailing edge. */
export function FabMenuContent({
  className,
  children,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Content>) {
  const { open } = useFabMenu()

  return (
    <AnimatePresence>
      {open && (
        <DropdownMenuPrimitive.Portal forceMount>
          <DropdownMenuPrimitive.Content
            side="top"
            align="end"
            sideOffset={16}
            {...props}
            asChild
          >
            <motion.div
              data-slot="fab-menu-content"
              className={cn(
                'z-50 flex flex-col items-end gap-1 outline-hidden',
                className,
              )}
              variants={contentVariants}
              initial="closed"
              animate="open"
              exit="closed"
            >
              {children}
            </motion.div>
          </DropdownMenuPrimitive.Content>
        </DropdownMenuPrimitive.Portal>
      )}
    </AnimatePresence>
  )
}

export interface FabMenuItemProps extends Omit<
  ComponentProps<typeof DropdownMenuPrimitive.Item>,
  'asChild'
> {
  icon: ReactNode
}

/** Selecting an item runs `onSelect` and then collapses the menu. */
export function FabMenuItem({
  icon,
  className,
  children,
  ...props
}: FabMenuItemProps) {
  return (
    <motion.div variants={itemVariants}>
      <DropdownMenuPrimitive.Item {...props} asChild>
        <Button
          variant="fab"
          data-slot="fab-menu-item"
          className={cn(
            'flex w-auto items-center gap-2 rounded-full pr-6 pl-4 whitespace-nowrap',
            'focus-visible:ring-primary focus-visible:ring-2',
            'data-highlighted:brightness-95 dark:data-highlighted:brightness-105',
            className,
          )}
        >
          <span
            data-slot="fab-menu-item-icon"
            className="grid size-6 shrink-0 place-content-center"
          >
            {icon}
          </span>

          <span data-slot="fab-menu-item-label">{children}</span>
        </Button>
      </DropdownMenuPrimitive.Item>
    </motion.div>
  )
}
