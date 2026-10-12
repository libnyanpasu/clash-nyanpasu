import MenuOpenRounded from '~icons/material-symbols/menu-open-rounded'
import { cva } from 'class-variance-authority'
import { motion, useReducedMotion } from 'motion/react'
import {
  useEffect,
  useRef,
  useState,
  type ComponentProps,
  type MouseEvent,
  type ReactElement,
  type ReactNode,
} from 'react'
import { cn } from '@nyanpasu/utils'
import { Button } from './button'
import { Ripple, type RippleConfig } from './ripple'
import { Sidebar, useSidebar } from './slider-sidebar'

export const NAVIGATION_SIDEBAR_WIDTH = { open: 200, closed: 88 } as const

export const navigationSidebarVariants = cva(
  'bg-surface z-10 flex min-h-0 shrink-0 flex-col max-md:inset-y-0 max-md:left-0 md:relative',
)

export const navigationSidebarItemVariants = cva(
  'group relative flex w-full min-w-0 shrink-0 overflow-hidden rounded-2xl p-0 text-on-surface-variant shadow-none transition-[height,background-color] duration-300 motion-reduce:transition-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary dark:text-on-surface-variant',
  {
    variants: {
      open: {
        true: [
          'h-11',
          'data-[active=true]:bg-secondary-container data-[active=true]:text-on-secondary-container',
          'dark:data-[active=true]:bg-secondary-container dark:data-[active=true]:text-on-secondary-container',
          'data-[active=false]:bg-transparent data-[active=false]:hover:bg-on-surface/8 dark:data-[active=false]:hover:bg-on-surface/8',
        ],
        false:
          'h-18 bg-transparent hover:bg-transparent data-[active=true]:text-on-secondary-container dark:bg-transparent dark:hover:bg-transparent dark:data-[active=true]:text-on-secondary-container',
      },
    },
  },
)

const navigationSidebarIconVariants = cva(
  'absolute grid shrink-0 place-items-center overflow-hidden rounded-full [&>svg]:size-6 [&>img]:size-6',
  {
    variants: {
      open: {
        true: '',
        false:
          'group-data-[active=true]:bg-secondary-container group-data-[active=false]:group-hover:bg-on-surface/8',
      },
    },
  },
)

/** Place inside `SidebarProvider` to render the animated navigation rail. */
export function NavigationSidebar({
  className,
  transition,
  ...props
}: Omit<ComponentProps<typeof Sidebar>, 'width'>) {
  const reducedMotion = useReducedMotion()

  return (
    <Sidebar
      className={cn(navigationSidebarVariants(), className)}
      width={NAVIGATION_SIDEBAR_WIDTH}
      transition={reducedMotion ? { type: 'tween', duration: 0 } : transition}
      data-slot="navigation-sidebar"
      {...props}
    />
  )
}

export type NavigationSidebarItemProps = Omit<
  ComponentProps<typeof Button>,
  'asChild' | 'children' | 'className' | 'disableRipple' | 'icon' | 'variant'
> & {
  active: boolean
  /** Hide the rail label for icon-only controls such as the sidebar toggle. */
  hideCollapsedLabel?: boolean
  children: ReactNode
  className?: string
  icon: ReactNode
  renderLink?: (content: ReactNode) => ReactElement
}

/**
 * Use inside `NavigationSidebar` under `SidebarProvider`; children are the
 * label. Keep routing in the caller by passing `renderLink` for a link item.
 */
export function NavigationSidebarItem({
  active,
  children,
  className,
  disabled,
  hideCollapsedLabel = false,
  icon,
  onClick,
  renderLink,
  ...props
}: NavigationSidebarItemProps) {
  const { open } = useSidebar()
  const reducedMotion = useReducedMotion()
  const transition = reducedMotion
    ? { duration: 0 }
    : { type: 'spring' as const, stiffness: 300, damping: 30 }
  const iconRef = useRef<HTMLSpanElement>(null)
  const nextRipple = useRef(0)
  const [ripples, setRipples] = useState<RippleConfig[]>([])

  useEffect(() => {
    setRipples([])
  }, [open])

  const handleClick = (event: MouseEvent<HTMLButtonElement>) => {
    const host = open ? event.currentTarget : iconRef.current
    if (host) {
      const rect = host.getBoundingClientRect()
      const pointerInsideHost =
        event.detail !== 0 &&
        event.clientX >= rect.left &&
        event.clientX <= rect.right &&
        event.clientY >= rect.top &&
        event.clientY <= rect.bottom
      const size = Math.max(rect.width, rect.height)
      const x = pointerInsideHost ? event.clientX - rect.left : rect.width / 2
      const y = pointerInsideHost ? event.clientY - rect.top : rect.height / 2
      const ripple: RippleConfig = {
        key: ++nextRipple.current,
        size,
        x: x - size / 2,
        y: y - size / 2,
      }
      setRipples((current) => [...current, ripple])
    }

    onClick?.(event)
  }

  const content = (
    <>
      <motion.span
        ref={iconRef}
        className={cn(navigationSidebarIconVariants({ open }))}
        data-slot="navigation-sidebar-icon"
        aria-hidden
        initial={false}
        animate={{
          x: open ? 16 : 8,
          y: open ? 10 : 6,
          width: open ? 24 : 56,
          height: open ? 24 : 32,
        }}
        style={{ top: 0, left: 0 }}
        transition={transition}
      >
        <span className="relative z-10 grid place-items-center [&>img]:size-6 [&>svg]:size-6">
          {icon}
        </span>
        {!open && (
          <Ripple
            ripples={ripples}
            onClear={(key) =>
              setRipples((current) =>
                current.filter((ripple) => ripple.key !== key),
              )
            }
          />
        )}
      </motion.span>

      <motion.span
        className="absolute z-10 overflow-hidden text-center text-xs leading-5 font-medium text-ellipsis whitespace-nowrap"
        data-slot="navigation-sidebar-label"
        aria-hidden={hideCollapsedLabel && !open}
        initial={false}
        animate={{
          x: open ? 52 : 0,
          y: open ? 12 : 44,
          width: open ? 116 : hideCollapsedLabel ? 0 : 72,
          opacity: hideCollapsedLabel && !open ? 0 : 1,
          fontSize: open ? 14 : 12,
        }}
        style={{ top: 0, left: 0, textAlign: open ? 'left' : 'center' }}
        transition={transition}
      >
        {children}
      </motion.span>

      {open && (
        <Ripple
          ripples={ripples}
          onClear={(key) =>
            setRipples((current) =>
              current.filter((ripple) => ripple.key !== key),
            )
          }
        />
      )}
    </>
  )

  return (
    <Button
      variant="basic"
      data-active={String(active)}
      className={cn(
        navigationSidebarItemVariants({ open }),
        hideCollapsedLabel && !open && 'h-11',
        className,
      )}
      onClick={handleClick}
      disabled={disabled}
      data-slot="navigation-sidebar-item"
      disableRipple
      asChild={Boolean(renderLink) && !disabled}
      {...props}
    >
      {renderLink && !disabled ? renderLink(content) : content}
    </Button>
  )
}

const navigationSidebarContentVariants = cva('flex flex-col gap-2 p-2')

export function NavigationSidebarContent({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(navigationSidebarContentVariants(), className)}
      {...props}
    />
  )
}

const navigationSidebarToggleVariants = cva(
  'absolute bottom-2 left-2 z-20 w-[calc(100%_-_1rem)]',
)

export function NavigationSidebarToggle({
  children,
  className,
  onClick,
  ...props
}: Omit<ComponentProps<typeof Button>, 'children'> & { children: string }) {
  const { open, setOpen } = useSidebar()

  return (
    <NavigationSidebarItem
      active
      hideCollapsedLabel
      icon={<MenuOpenRounded />}
      className={cn(navigationSidebarToggleVariants(), className)}
      aria-expanded={open}
      data-slot="navigation-sidebar-toggle"
      onClick={(event) => {
        onClick?.(event)
        if (!event.defaultPrevented) setOpen(!open)
      }}
      {...props}
    >
      {children}
    </NavigationSidebarItem>
  )
}
