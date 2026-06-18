import { cva, type VariantProps } from 'class-variance-authority'
import { motion, useReducedMotion } from 'framer-motion'
import { Slot } from 'radix-ui'
import {
  cloneElement,
  ComponentProps,
  createContext,
  isValidElement,
  use,
  useCallback,
  useId,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { cn } from '@nyanpasu/utils'

const containerVariants = cva('relative inline-flex items-stretch', {
  variants: {
    variant: {
      pill: [
        'rounded-full',
        'bg-surface-variant/40 dark:bg-surface-variant/15',
      ],
      segment: [
        'rounded-full overflow-hidden',
        'border border-outline dark:border-outline-variant',
        'divide-x divide-outline dark:divide-outline-variant',
        'bg-transparent-fallback-surface dark:bg-transparent-fallback-surface-variant',
      ],
    },
    size: {
      sm: 'h-9 text-xs',
      md: 'h-11 text-sm',
      lg: 'h-13 text-base',
    },
  },
  defaultVariants: {
    variant: 'pill',
    size: 'md',
  },
})

type AnimatedTabContainer = VariantProps<typeof containerVariants>

const tabVariants = cva(
  [
    'group relative',
    'flex min-w-0 flex-1 items-center justify-center',
    'cursor-pointer select-none',
    'font-medium outline-hidden',
    'focus-visible:outline-none focus-visible:ring-2',
    'focus-visible:ring-ring focus-visible:ring-offset-2',
  ],
  {
    variants: {
      variant: {
        pill: [
          'rounded-full px-4',
          'text-on-surface-variant dark:text-on-surface-variant',
          'aria-selected:text-on-secondary-container dark:aria-selected:text-on-secondary-container',
          'transition-colors duration-200',
        ],
        segment: [
          'px-3',
          'text-on-surface dark:text-on-surface',
          'transition-[background-color,color,filter]',
          'hover:bg-on-surface/8 dark:hover:bg-on-surface/12',
          'focus-visible:bg-on-surface/10',
          'aria-selected:text-on-secondary-container dark:aria-selected:text-on-secondary-container',
          'aria-selected:hover:brightness-95 dark:aria-selected:hover:brightness-105',
        ],
      },
    },
    defaultVariants: {
      variant: 'pill',
    },
  },
)

const activeIndicatorVariants = cva(
  [
    'absolute inset-0 z-1',
    'bg-secondary-container dark:bg-secondary-container',
  ],
  {
    variants: {
      variant: {
        pill: 'rounded-full',
        segment: '',
      },
    },
    defaultVariants: {
      variant: 'pill',
    },
  },
)

const AnimatedTabsContext = createContext<{
  activeTab: string
  layoutId: string
  onKeyDown: (event: React.KeyboardEvent, tabId?: string) => void
  onTabChange: (tabId?: string) => void
  shouldReduceMotion: boolean | null
  variant: AnimatedTabContainer['variant']
} | null>(null)

function useAnimatedTabsContext() {
  const ctx = use(AnimatedTabsContext)

  if (!ctx) {
    throw new Error('AnimatedTabsItem must be used within AnimatedTabs')
  }

  return ctx
}

const SPRING = {
  type: 'spring' as const,
  duration: 0.35,
  bounce: 0.15,
}

export function AnimatedTabsItem({
  value,
  isActive: isActiveProp,
  className,
  children,
  asChild,
  ...props
}: ComponentProps<'button'> & {
  value?: string
  isActive?: boolean
  asChild?: boolean
}) {
  const {
    activeTab,
    variant,
    layoutId,
    shouldReduceMotion,
    onTabChange,
    onKeyDown,
  } = useAnimatedTabsContext()

  const isActive = isActiveProp ?? activeTab === value

  const Comp = asChild ? Slot.Root : 'button'

  const { id, onClick, onKeyDown: onKeyDownProp, type, ...restProps } = props

  const tabValue = value || 'null'

  const tabId = id ?? `${layoutId}-tab-${tabValue}`

  const renderLabel = (content: ReactNode) => (
    <span
      className="relative z-3 flex items-center gap-2"
      data-slot="animated-tabs-item-label"
    >
      {content}
    </span>
  )

  const handleClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    onClick?.(event)

    if (!event.defaultPrevented) {
      onTabChange(value)
    }
  }

  const handleItemKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    onKeyDownProp?.(event)

    if (!event.defaultPrevented) {
      onKeyDown(event, value)
    }
  }

  let slottableChild: ReactNode

  if (asChild) {
    if (!isValidElement(children)) {
      throw new Error(
        'AnimatedTabsItem with asChild expects a single React element child',
      )
    }

    const childContent = (children.props as { children?: ReactNode }).children

    slottableChild = cloneElement(
      children,
      undefined,
      renderLabel(childContent),
    )
  } else {
    slottableChild = renderLabel(children)
  }

  return (
    <Comp
      aria-selected={isActive}
      className={cn(tabVariants({ variant }), className)}
      data-slot="animated-tabs-item"
      data-tab-id={tabValue}
      id={tabId}
      onClick={handleClick}
      onKeyDown={handleItemKeyDown}
      role="tab"
      tabIndex={isActive ? 0 : -1}
      type={asChild ? undefined : (type ?? 'button')}
      {...restProps}
    >
      {/* Sliding pill indicator — rendered first so it sits at the bottom of the stack */}
      {isActive && (
        <motion.span
          aria-hidden
          className={activeIndicatorVariants({
            variant,
          })}
          layout
          layoutId={layoutId}
          transition={shouldReduceMotion ? { duration: 0 } : SPRING}
          data-slot="animated-tabs-indicator"
        />
      )}

      {/* Hover state layer for pill — sits above the indicator */}
      {variant === 'pill' && (
        <span
          aria-hidden
          className={cn(
            'absolute inset-0 z-2 rounded-full',
            'opacity-0 transition-opacity duration-150',
            'bg-on-surface dark:bg-on-surface',
            'group-focus-visible:opacity-10',
          )}
          data-slot="animated-tabs-hover-layer"
        />
      )}

      <Slot.Slottable>{slottableChild}</Slot.Slottable>
    </Comp>
  )
}

export default function AnimatedTabs({
  children,
  activeTab: controlledActiveTab,
  defaultTab,
  onChange,
  variant = 'pill',
  size,
  className,
}: {
  activeTab?: string
  children: ReactNode
  className?: string
  defaultTab?: string
  onChange?: (tabId: string) => void
  variant?: AnimatedTabContainer['variant']
  size?: AnimatedTabContainer['size']
}) {
  const shouldReduceMotion = useReducedMotion()

  const layoutId = useId()

  const containerRef = useRef<HTMLDivElement>(null)

  const [internalActiveTab, setInternalActiveTab] = useState(defaultTab ?? '')

  const isControlled = controlledActiveTab !== undefined

  const activeTab = isControlled ? controlledActiveTab : internalActiveTab

  const handleTabChange = useCallback(
    (tabId?: string) => {
      if (!isControlled) {
        setInternalActiveTab(tabId || 'null')
      }
      onChange?.(tabId || 'null')
    },
    [isControlled, onChange],
  )

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent, tabId?: string) => {
      if (!containerRef.current) return
      const tabElements = Array.from(
        containerRef.current.querySelectorAll<HTMLElement>('[role="tab"]'),
      )
      const currentIndex = tabElements.findIndex(
        (el) => el.dataset.tabId === (tabId || 'null'),
      )
      let newIndex = currentIndex

      if (event.key === 'ArrowRight') {
        event.preventDefault()
        newIndex = (currentIndex + 1) % tabElements.length
      } else if (event.key === 'ArrowLeft') {
        event.preventDefault()
        newIndex = (currentIndex - 1 + tabElements.length) % tabElements.length
      } else if (event.key === 'Home') {
        event.preventDefault()
        newIndex = 0
      } else if (event.key === 'End') {
        event.preventDefault()
        newIndex = tabElements.length - 1
      } else {
        return
      }

      const newTabEl = tabElements[newIndex]
      if (newTabEl) {
        const newTabId = newTabEl.dataset.tabId
        handleTabChange(newTabId)
        newTabEl.focus()
      }
    },
    [handleTabChange],
  )

  return (
    <AnimatedTabsContext.Provider
      value={{
        activeTab,
        layoutId,
        onKeyDown: handleKeyDown,
        onTabChange: handleTabChange,
        shouldReduceMotion,
        variant,
      }}
    >
      <div
        aria-label="Tabs"
        className={cn(
          containerVariants({
            variant,
            size,
          }),
          className,
        )}
        data-slot="animated-tabs-container"
        ref={containerRef}
        role="tablist"
      >
        {children}
      </div>
    </AnimatedTabsContext.Provider>
  )
}
