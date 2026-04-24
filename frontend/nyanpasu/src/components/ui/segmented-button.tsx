import Check from '~icons/material-symbols/check-rounded'
import { cva, type VariantProps } from 'class-variance-authority'
import { motion } from 'framer-motion'
import { ToggleGroup as ToggleGroupPrimitive } from 'radix-ui'
import {
  ComponentProps,
  createContext,
  lazy,
  Suspense,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { chains } from '@/utils/chain'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { useRipple } from './ripple'

export const segmentedButtonVariants = cva(
  'relative inline-flex w-full items-stretch',
  {
    variants: {
      variant: {
        filled: [
          'rounded-full overflow-hidden',
          'border border-outline dark:border-outline-variant',
          'divide-x divide-outline dark:divide-outline-variant',
          'bg-transparent-fallback-surface dark:bg-transparent-fallback-surface-variant',
          'transition-opacity',
        ],
        tabs: [
          'rounded-full',
          'bg-surface-variant/40 dark:bg-surface-variant/15',
          'transition-opacity',
        ],
      },
      size: {
        sm: 'h-9 text-xs',
        md: 'h-11 text-sm',
        lg: 'h-13 text-base',
      },
      disabled: {
        true: 'cursor-not-allowed opacity-50',
        false: '',
      },
    },
    defaultVariants: {
      variant: 'filled',
      size: 'md',
      disabled: false,
    },
  },
)

export type SegmentedButtonVariantsProps = VariantProps<
  typeof segmentedButtonVariants
>

export const segmentedButtonItemVariants = cva(
  [
    'group relative overflow-hidden',
    'flex min-w-0 flex-1 items-center justify-center',
    'cursor-pointer select-none',
    'font-medium outline-hidden',
    'transition-[color,filter]',
    'data-disabled:cursor-not-allowed data-disabled:text-on-surface/40',
  ],
  {
    variants: {
      variant: {
        filled: [
          'px-3',
          'text-on-surface dark:text-on-surface',
          'transition-[background-color,color,filter]',
          'hover:bg-on-surface/8 dark:hover:bg-on-surface/12',
          'focus-visible:bg-on-surface/10',
          'data-disabled:hover:bg-transparent',
          'data-[state=on]:bg-secondary-container dark:data-[state=on]:bg-secondary-container',
          'data-[state=on]:text-on-secondary-container dark:data-[state=on]:text-on-secondary-container',
          'data-[state=on]:hover:brightness-95 dark:data-[state=on]:hover:brightness-105',
        ],
        tabs: [
          'rounded-full px-4',
          'text-on-surface-variant dark:text-on-surface-variant',
          'data-[state=on]:text-on-secondary-container dark:data-[state=on]:text-on-secondary-container',
        ],
      },
    },
    defaultVariants: {
      variant: 'filled',
    },
  },
)

export type SegmentedButtonItemVariantsProps = VariantProps<
  typeof segmentedButtonItemVariants
>

const LazyRipple = lazy(() =>
  import('./ripple').then((mod) => ({ default: mod.Ripple })),
)

type SegmentedButtonVariant = NonNullable<
  SegmentedButtonVariantsProps['variant']
>

type SegmentedValue = string | undefined

type PillRect = { x: number; width: number } | null

const SegmentedButtonContext = createContext<{
  variant: SegmentedButtonVariant
  currentValue: SegmentedValue
  registerItemRef: (value: string, el: HTMLButtonElement | null) => void
} | null>(null)

const useSegmentedButtonContext = () => {
  const context = useContext(SegmentedButtonContext)

  if (context === null) {
    throw new Error(
      'SegmentedButton compound components cannot be rendered outside the SegmentedButton component',
    )
  }

  return context
}

type ToggleGroupRootProps = ComponentProps<typeof ToggleGroupPrimitive.Root>

type SegmentedButtonRootProps = Omit<
  ToggleGroupRootProps,
  'type' | 'value' | 'defaultValue' | 'onValueChange'
> & {
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
}

export type SegmentedButtonProps = SegmentedButtonRootProps &
  SegmentedButtonVariantsProps

export const SegmentedButton = ({
  className,
  variant,
  size,
  disabled,
  value,
  defaultValue,
  onValueChange,
  children,
  ...rest
}: SegmentedButtonProps) => {
  const rootRef = useRef<HTMLDivElement>(null)
  const itemRefs = useRef(new Map<string, HTMLButtonElement>())
  const [pillRect, setPillRect] = useState<PillRect>(null)
  const [hasMeasured, setHasMeasured] = useState(false)

  const handleControllableChange = useCallback(
    (nextValue: SegmentedValue) => {
      if (typeof nextValue === 'string') {
        onValueChange?.(nextValue)
      }
    },
    [onValueChange],
  )

  const [currentValue, setCurrentValue] = useControllableState<SegmentedValue>({
    prop: value,
    defaultProp: defaultValue,
    onChange: handleControllableChange,
  })

  const handleValueChange = useCallback(
    (nextValue: string) => {
      setCurrentValue(nextValue)
    },
    [setCurrentValue],
  )

  const resolvedVariant: SegmentedButtonVariant = variant ?? 'filled'

  const registerItemRef = useCallback(
    (itemValue: string, el: HTMLButtonElement | null) => {
      if (el) {
        itemRefs.current.set(itemValue, el)
      } else {
        itemRefs.current.delete(itemValue)
      }
    },
    [],
  )

  const measurePill = useCallback(() => {
    if (resolvedVariant !== 'tabs') {
      setPillRect(null)
      return
    }

    if (!currentValue || Array.isArray(currentValue)) {
      setPillRect(null)
      return
    }

    const target = itemRefs.current.get(currentValue)
    const root = rootRef.current
    if (!target || !root) {
      setPillRect(null)
      return
    }

    const itemBox = target.getBoundingClientRect()
    const rootBox = root.getBoundingClientRect()

    setPillRect({
      x: itemBox.left - rootBox.left,
      width: itemBox.width,
    })
  }, [resolvedVariant, currentValue])

  useLayoutEffect(() => {
    measurePill()
  }, [measurePill])

  useEffect(() => {
    const root = rootRef.current
    if (!root || resolvedVariant !== 'tabs') return

    const observer = new ResizeObserver(() => measurePill())
    observer.observe(root)
    itemRefs.current.forEach((el) => observer.observe(el))

    return () => observer.disconnect()
  }, [measurePill, resolvedVariant])

  const showSliderPill =
    resolvedVariant === 'tabs' && pillRect !== null && !!currentValue

  return (
    <SegmentedButtonContext.Provider
      value={{
        variant: resolvedVariant,
        currentValue,
        registerItemRef,
      }}
    >
      <ToggleGroupPrimitive.Root
        ref={rootRef}
        type="single"
        className={cn(
          segmentedButtonVariants({
            variant,
            size,
            disabled,
          }),
          className,
        )}
        disabled={disabled ?? undefined}
        value={currentValue}
        onValueChange={handleValueChange}
        {...rest}
      >
        {showSliderPill && (
          <motion.div
            aria-hidden
            data-slot="segmented-button-slider-pill"
            className={cn(
              'pointer-events-none absolute top-0 bottom-0 left-0',
              'bg-secondary-container dark:bg-secondary-container rounded-full',
            )}
            initial={false}
            animate={{ x: pillRect!.x, width: pillRect!.width }}
            transition={
              hasMeasured
                ? { type: 'spring', bounce: 0.15, duration: 0.45 }
                : { duration: 0 }
            }
            onAnimationStart={() => {
              if (!hasMeasured) setHasMeasured(true)
            }}
          />
        )}

        {children}
      </ToggleGroupPrimitive.Root>
    </SegmentedButtonContext.Provider>
  )
}

export interface SegmentedButtonItemProps extends ComponentProps<
  typeof ToggleGroupPrimitive.Item
> {
  /**
   * Hide the automatic check indicator shown when the item is selected.
   * Only relevant in the `filled` variant �?tabs never render the check.
   * Use this when providing a persistent leading icon via `children`.
   */
  hideIndicator?: boolean
}

export const SegmentedButtonItem = ({
  className,
  children,
  hideIndicator,
  onClick,
  disabled,
  value,
  ...props
}: SegmentedButtonItemProps) => {
  const { variant, registerItemRef } = useSegmentedButtonContext()

  const ripple = useRipple()

  const handleClick = disabled ? undefined : chains(onClick, ripple.onClick)

  const handleClear = useCallback(
    (key: React.Key) => {
      ripple.onClear(key)
    },
    [ripple],
  )

  const setRef = useCallback(
    (el: HTMLButtonElement | null) => {
      registerItemRef(value, el)
    },
    [registerItemRef, value],
  )

  const showCheck = variant === 'filled' && !hideIndicator

  return (
    <ToggleGroupPrimitive.Item
      ref={setRef}
      className={cn(segmentedButtonItemVariants({ variant }), className)}
      disabled={disabled}
      onClick={handleClick}
      value={value}
      {...props}
    >
      {variant === 'tabs' && (
        <span
          aria-hidden
          data-slot="segmented-button-item-state-layer"
          className={cn(
            'absolute inset-0 rounded-full',
            'opacity-0 transition-opacity duration-150',
            'bg-on-surface dark:bg-on-surface',
            'group-hover:opacity-[0.08] dark:group-hover:opacity-[0.12]',
            'group-focus-visible:opacity-[0.1]',
            'group-data-disabled:group-hover:opacity-0',
          )}
        />
      )}

      <span
        data-slot="segmented-button-item-content"
        className={cn(
          'relative z-10 flex max-w-full min-w-0 items-center justify-center gap-2',
        )}
      >
        {showCheck && (
          <Check
            aria-hidden
            className={cn(
              'size-4 shrink-0',
              '-ml-5 scale-75 opacity-0',
              'transition-[opacity,margin,transform] duration-200',
              'group-data-[state=on]:ml-0',
              'group-data-[state=on]:scale-100',
              'group-data-[state=on]:opacity-100',
            )}
          />
        )}

        <span className="truncate">{children}</span>
      </span>

      <Suspense>
        {ripple && !disabled && (
          <LazyRipple ripples={ripple.ripples} onClear={handleClear} />
        )}
      </Suspense>
    </ToggleGroupPrimitive.Item>
  )
}
