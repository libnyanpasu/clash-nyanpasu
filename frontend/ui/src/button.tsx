import { cva, type VariantProps } from 'class-variance-authority'
import { AnimatePresence, motion } from 'motion/react'
import { Slot } from 'radix-ui'
import {
  cloneElement,
  isValidElement,
  lazy,
  PropsWithChildren,
  Suspense,
  useCallback,
  useContext,
  type ReactNode,
} from 'react'
import { chains, cn } from '@nyanpasu/utils'
import { GlassControlGroupContext, glassSurfaceVariants } from './glass-surface'
import { CircularProgress } from './progress'
import { useRipple } from './ripple'

export const buttonVariants = cva(
  [
    'cursor-pointer select-none',
    'focus:outline-hidden',
    'relative overflow-hidden',
    'h-10 text-sm font-medium',
    'rounded-full',
    'transition-[background-color,color,shadow,filter]',
  ],
  {
    variants: {
      variant: {
        basic: [
          'px-4',
          'text-primary',
          'bg-transparent-fallback-surface',
          'hover:bg-on-surface/8',
        ],
        raised: [
          'px-6',
          'text-primary',
          'shadow-xs hover:shadow-sm focus:shadow-sm',
          'bg-surface-container-low',
          'hover:bg-surface-container',
        ],
        glass: [
          glassSurfaceVariants(),
          'h-11 px-6 text-on-surface-variant',
          'hover:bg-surface-container-highest',
        ],
        stroked: [
          'px-6',
          'text-primary',
          'border border-outline',
          'bg-transparent-fallback-surface',
          'hover:bg-on-surface/8',
        ],
        flat: [
          'px-6',
          'text-on-primary',
          'bg-primary',
          'hover:brightness-95 dark:hover:brightness-105',
        ],
        fab: [
          'px-4 h-14',
          'rounded-2xl',
          'shadow-sm',
          'text-on-primary-container',
          'bg-primary-container',
          'hover:shadow-md',
          'hover:brightness-95 dark:hover:brightness-105',
        ],
      },
      disabled: {
        true: 'cursor-not-allowed shadow-none hover:shadow-none focus:shadow-none',
        false: '',
      },
      icon: {
        true: 'p-0 grid place-content-center',
        false: 'min-w-16',
      },
    },
    compoundVariants: [
      {
        variant: 'basic',
        disabled: true,
        className: 'bg-transparent text-on-surface/38 hover:bg-transparent',
      },
      {
        variant: 'raised',
        disabled: true,
        className: 'bg-on-surface/12 text-on-surface/38 hover:bg-on-surface/12',
      },
      {
        variant: 'stroked',
        disabled: true,
        className:
          'border-on-surface/12 bg-transparent text-on-surface/38 hover:bg-transparent',
      },
      {
        variant: 'flat',
        disabled: true,
        className:
          'bg-on-surface/12 text-on-surface/38 hover:bg-on-surface/12 hover:brightness-100',
      },
      {
        variant: 'fab',
        disabled: true,
        className:
          'bg-on-surface/12 text-on-surface/38 hover:bg-on-surface/12 hover:brightness-100 hover:shadow-none',
      },
      {
        icon: true,
        className: 'w-10',
      },
      {
        variant: 'fab',
        icon: true,
        className: 'w-14',
      },
      {
        variant: 'glass',
        icon: true,
        className: 'w-11',
      },
    ],
    defaultVariants: {
      variant: 'basic',
      disabled: false,
      icon: false,
    },
  },
)

export type ButtonVariantsProps = VariantProps<typeof buttonVariants>

const glassGroupButtonVariants = cva('', {
  variants: {
    icon: {
      true: null,
      false: 'flex h-9 min-w-0 shrink-0 items-center gap-1 px-3',
    },
  },
  defaultVariants: {
    icon: false,
  },
})

const LazyRipple = lazy(() =>
  import('./ripple').then((mod) => ({ default: mod.Ripple })),
)

export interface ButtonProps
  extends
    Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'disabled'>,
    ButtonVariantsProps {
  asChild?: boolean
  loading?: boolean
  disableRipple?: boolean
}

export const Button = ({
  loading,
  asChild,
  variant,
  disabled,
  icon,
  className,
  children,
  onClick,
  disableRipple = false,
  ...props
}: ButtonProps) => {
  const Comp = asChild ? Slot.Root : 'button'
  const glassGroup = useContext(GlassControlGroupContext)
  const groupedGlass = variant === 'glass' && glassGroup

  const content = (value: ReactNode) => (
    <span className="contents" data-slot="button-content">
      {value}
    </span>
  )
  const buttonChildren =
    asChild && isValidElement<PropsWithChildren>(children)
      ? cloneElement(children, {}, content(children.props.children))
      : content(children)

  const ripple = useRipple()

  const handleClick = disabled
    ? undefined
    : disableRipple
      ? onClick
      : chains(onClick, ripple.onClick)

  const handleClear = useCallback(
    (key: React.Key) => {
      ripple.onClear(key)
    },
    [ripple],
  )

  return (
    <Comp
      className={cn(
        buttonVariants({
          variant: groupedGlass ? 'basic' : variant,
          disabled,
          icon,
        }),
        groupedGlass && glassGroupButtonVariants({ icon }),
        '[&:has(>[data-slot=button-loading])>[data-slot=button-content]]:invisible',
        className,
      )}
      onClick={handleClick}
      disabled={disabled ?? undefined}
      data-loading={String(Boolean(loading))}
      {...props}
    >
      <Slot.Slottable>{buttonChildren}</Slot.Slottable>

      <AnimatePresence initial={false}>
        {loading && (
          <motion.span
            className={cn(
              'absolute inset-0 z-10 flex h-full w-full cursor-wait items-center justify-center',
              'bg-inherit-allow-fallback',
            )}
            data-slot="button-loading"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <CircularProgress className="size-3/5" indeterminate />
          </motion.span>
        )}
      </AnimatePresence>

      <Suspense>
        {ripple && !disableRipple && !loading && !disabled && (
          <LazyRipple ripples={ripple.ripples} onClear={handleClear} />
        )}
      </Suspense>
    </Comp>
  )
}
