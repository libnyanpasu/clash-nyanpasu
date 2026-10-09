import CheckRounded from '~icons/material-symbols/check-rounded'
import { motion, useReducedMotion } from 'motion/react'
import type { ReactNode } from 'react'
import { cn } from '@nyanpasu/utils'
import { Button, type ButtonProps } from './button'
import { MaterialShape } from './material-shape'
import { CircularProgress } from './progress'

export function ShapeToggle({
  active,
  icon,
  label,
  loading,
  disabled,
  className,
  compact = false,
  elastic = false,
  ...props
}: Omit<ButtonProps, 'children' | 'icon'> & {
  active: boolean
  icon: ReactNode
  label: string
  compact?: boolean
  elastic?: boolean
}) {
  const reducedMotion = useReducedMotion()
  return (
    <Button
      {...props}
      asChild
      variant="basic"
      disableRipple
      disabled={disabled || loading}
      className={cn(
        'group focus-visible:ring-primary [container-type:size] flex h-full min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-1.5 rounded-2xl bg-transparent p-1 shadow-none [container-name:shape-toggle] hover:bg-transparent focus-visible:ring-2 dark:bg-transparent dark:hover:bg-transparent',
        className,
        compact && 'flex-row justify-start gap-3',
        elastic &&
          'relative isolate overflow-hidden rounded-2xl px-2 py-1 transition-colors duration-200',
        elastic &&
          (active
            ? 'bg-primary-container text-on-primary-container hover:bg-primary-container dark:bg-primary-container dark:text-on-primary-container dark:hover:bg-primary-container'
            : 'bg-surface-variant/35 text-on-surface-variant hover:bg-surface-variant/50 dark:bg-surface-variant/35 dark:text-on-surface-variant dark:hover:bg-surface-variant/50'),
      )}
    >
      <motion.button
        type="button"
        role="switch"
        aria-checked={active}
        aria-label={label}
        aria-busy={loading}
        data-slot="shape-toggle"
        initial={false}
        animate={{ flexGrow: elastic && active ? 1.65 : 1 }}
        transition={{
          duration: reducedMotion ? 0 : 0.38,
          ease: [0.22, 1, 0.36, 1],
        }}
      >
        <motion.span
          data-slot="shape-toggle-surface"
          data-compact={compact}
          className={cn(
            '@container-size relative grid shrink-0 place-items-center',
            elastic
              ? 'size-11 [@container_shape-toggle_(max-height:110px)]:size-8'
              : compact
                ? 'size-12'
                : 'aspect-square h-[min(5rem,calc(100%-2.25rem))] max-w-full [@container_shape-toggle_(max-height:110px)]:h-[min(3.5rem,calc(100%-1.75rem))]',
          )}
          initial={false}
          animate={{
            scale: reducedMotion
              ? 1
              : active
                ? [0.96, 1.025, 1]
                : elastic
                  ? 1
                  : [0.98, 1.01, 1],
          }}
          whileTap={reducedMotion ? undefined : { scale: 0.94 }}
          transition={{
            duration: reducedMotion ? 0 : 0.38,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          <MaterialShape
            active={active}
            depth={active ? 8 : 5}
            className={cn(
              'absolute inset-0 transition-[color,opacity] duration-200',
              elastic
                ? active
                  ? 'bg-primary opacity-100'
                  : 'bg-primary opacity-0'
                : active
                  ? 'bg-primary'
                  : 'bg-surface-variant/60',
            )}
          >
            <span className="absolute inset-0 bg-current opacity-0 transition-opacity group-hover:opacity-8 group-active:opacity-14" />
          </MaterialShape>
          <motion.span
            initial={false}
            animate={{ y: active && !loading && !elastic ? -3 : 0 }}
            className={cn(
              'relative grid aspect-square w-[clamp(1.25rem,40cqi,1.75rem)] place-items-center',
              active ? 'text-on-primary' : 'text-on-surface-variant',
            )}
            transition={{ duration: reducedMotion ? 0 : 0.3 }}
          >
            {loading ? (
              <CircularProgress
                className="size-full"
                aria-hidden
                indeterminate
              />
            ) : (
              <span className="grid size-full place-items-center [&_svg]:size-full">
                {icon}
              </span>
            )}
          </motion.span>
          <motion.span
            className={cn(
              'text-on-primary absolute',
              'bottom-[15%] w-[clamp(0.5rem,12cqi,0.75rem)]',
              elastic && 'hidden',
            )}
            initial={false}
            animate={{
              opacity: active && !loading ? 1 : 0,
              scale: active && !loading ? 1 : 0.6,
            }}
            transition={{ duration: reducedMotion ? 0 : 0.2 }}
          >
            <CheckRounded className="size-full" aria-hidden />
          </motion.span>
        </motion.span>
        {elastic && (
          <motion.span
            aria-hidden
            className="text-on-primary-container absolute top-2 right-2"
            initial={false}
            animate={{
              opacity: active && !loading ? 1 : 0,
              scale: active && !loading ? 1 : 0.7,
            }}
            transition={{ duration: reducedMotion ? 0 : 0.2 }}
          >
            <CheckRounded className="size-3" />
          </motion.span>
        )}
        <span
          className={cn(
            'max-w-full shrink-0 truncate text-xs leading-4 font-medium [@container_shape-toggle_(max-height:110px)]:text-[11px]',
            elastic ? 'text-inherit' : 'text-on-surface',
          )}
          data-slot="shape-toggle-label"
        >
          {label}
        </span>
      </motion.button>
    </Button>
  )
}
