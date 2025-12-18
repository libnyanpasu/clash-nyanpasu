import { cva, type VariantProps } from 'class-variance-authority'
import { AnimatePresence, motion } from 'framer-motion'
import { lazy, Suspense, useCallback } from 'react'
import { chains } from '@/utils/chain'
import { cn } from '@nyanpasu/ui'
import { Slot } from '@radix-ui/react-slot'
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
          'text-primary dark:text-primary',
          'bg-transparent-fallback-surface dark:bg-transparent-fallback-on-surface',
          'hover:bg-primary-container dark:hover:bg-surface-variant',
        ],
        raised: [
          'px-6',
          'text-primary dark:text-on-surface',
          'shadow-sm hover:shadow-lg focus:shadow-xl',
          'bg-surface',
          'hover:bg-surface-variant',
        ],
        stroked: [
          'px-6',
          'text-primary',
          'border border-primary',
          'bg-transparent-fallback-surface dark:bg-transparent-fallback-on-surface',
          'hover:bg-primary-container dark:hover:bg-surface-variant',
        ],
        flat: [
          'px-6',
          'text-surface dark:text-on-surface',
          'bg-primary dark:bg-primary-container',
          'dark:hover:bg-on-primary',
        ],
        fab: [
          'px-4 h-14',
          'rounded-2xl',
          'shadow-xl',
          'text-on-primary-container dark:text-on-primary-container',
          'bg-primary-container dark:bg-on-primary',
          'hover:shadow-2xl',
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
        className: 'text-zinc-900/40 hover:bg-transparent',
      },
      {
        variant: 'raised',
        disabled: true,
        className: 'bg-gray-900/20 text-zinc-900/40 hover:bg-gray-900/20',
      },
      {
        variant: 'stroked',
        disabled: true,
        className: 'text-zinc-900/40 hover:bg-transparent border-zinc-300',
      },
      {
        variant: 'flat',
        disabled: true,
        className: 'bg-gray-900/20 text-gray-900/40 hover:bg-primary',
      },
      {
        variant: 'fab',
        disabled: true,
        className:
          'bg-gray-900/20 text-gray-900/40 hover:brightness-100 hover:shadow-container-xl',
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
    ],
    defaultVariants: {
      variant: 'basic',
      disabled: false,
      icon: false,
    },
  },
)

export type ButtonVariantsProps = VariantProps<typeof buttonVariants>

const LazyRipple = lazy(() =>
  import('./ripple').then((mod) => ({ default: mod.Ripple })),
)

export interface ButtonProps
  extends
    Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'disabled'>,
    ButtonVariantsProps {
  asChild?: boolean
  loading?: boolean
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
  ...props
}: ButtonProps) => {
  const Comp = asChild ? Slot : 'button'

  const ripple = useRipple()

  const handleClick = disabled ? undefined : chains(onClick, ripple.onClick)

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
          variant,
          disabled,
          icon,
        }),
        className,
      )}
      onClick={handleClick}
      data-loading={String(Boolean(loading))}
      {...props}
    >
      {asChild ? (
        children
      ) : (
        <>
          {children}

          <AnimatePresence initial={false}>
            {loading && (
              <motion.span
                className={cn(
                  'absolute inset-0 flex h-full w-full cursor-wait items-center justify-center',
                  'bg-inherit-allow-fallback',
                )}
                data-slot="button-loading"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
              >
                <CircularProgress className="size-6" indeterminate />
              </motion.span>
            )}
          </AnimatePresence>

          <Suspense>
            {ripple && !loading && !disabled && (
              <LazyRipple ripples={ripple.ripples} onClear={handleClear} />
            )}
          </Suspense>
        </>
      )}
    </Comp>
  )
}
