import { cva, type VariantProps } from 'class-variance-authority'
import { motion, useReducedMotion } from 'framer-motion'
import {
  ComponentProps,
  createContext,
  use,
  useCallback,
  useId,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { cn } from '@nyanpasu/utils'

export type AnimatedTabVariant = 'pill' | 'segment'

const AnimatedTabsContext = createContext<{
  activeTab: string
  layoutId: string
  onKeyDown: (event: React.KeyboardEvent, tabId: string) => void
  onTabChange: (tabId: string) => void
  shouldReduceMotion: boolean | null
  variant: AnimatedTabVariant
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

export function AnimatedTabsItem({
  value,
  className,
  children,
  ...props
}: ComponentProps<'button'> & {
  value: string
}) {
  const {
    activeTab,
    variant,
    layoutId,
    shouldReduceMotion,
    onTabChange,
    onKeyDown,
  } = useAnimatedTabsContext()

  const isActive = activeTab === value

  return (
    <button
      aria-selected={isActive}
      className={cn(tabVariants({ variant }), className)}
      data-slot="animated-tabs-item"
      onClick={() => onTabChange(value)}
      onKeyDown={(e) => onKeyDown(e, value)}
      role="tab"
      tabIndex={isActive ? 0 : -1}
      type="button"
      {...props}
    >
      {/* Sliding pill indicator — rendered first so it sits at the bottom of the stack */}
      {isActive && (
        <motion.span
          aria-hidden
          className={cn(
            'absolute inset-0 z-1',
            variant === 'pill' &&
              'bg-secondary-container dark:bg-secondary-container rounded-full',
            variant === 'segment' &&
              'bg-secondary-container dark:bg-secondary-container',
          )}
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

      <span className="relative z-3 flex items-center gap-2">{children}</span>
    </button>
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
  variant?: AnimatedTabVariant
  size?: VariantProps<typeof containerVariants>['size']
}) {
  const shouldReduceMotion = useReducedMotion()

  const layoutId = useId()

  const containerRef = useRef<HTMLDivElement>(null)

  const [internalActiveTab, setInternalActiveTab] = useState(defaultTab ?? '')

  const isControlled = controlledActiveTab !== undefined

  const activeTab = isControlled ? controlledActiveTab : internalActiveTab

  const handleTabChange = useCallback(
    (tabId: string) => {
      if (!isControlled) {
        setInternalActiveTab(tabId)
      }
      onChange?.(tabId)
    },
    [isControlled, onChange],
  )

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent, tabId: string) => {
      if (!containerRef.current) return
      const tabElements = Array.from(
        containerRef.current.querySelectorAll<HTMLButtonElement>(
          '[role="tab"]',
        ),
      )
      const currentIndex = tabElements.findIndex(
        (el) => el.id === `${layoutId}-tab-${tabId}`,
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
        const newTabId = newTabEl.id.replace(`${layoutId}-tab-`, '')
        handleTabChange(newTabId)
        newTabEl.focus()
      }
    },
    [layoutId, handleTabChange],
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
