import CheckRounded from '~icons/material-symbols/check-rounded'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useEffect, useRef, type ReactNode } from 'react'
import { cn } from '@nyanpasu/utils'
import { Button } from './button'
import { MaterialShape } from './material-shape'

export type ExpressiveChoiceOption = {
  value: string
  label: string
  icon: ReactNode
  description?: string
}

export function ExpressiveChoice({
  options,
  value,
  layout,
  disabled,
  label,
  onChange,
}: {
  options: ExpressiveChoiceOption[]
  value: string | null
  layout: 'focus' | 'flex'
  disabled?: boolean
  label: string
  onChange?: (value: string) => void
}) {
  const reducedMotion = useReducedMotion()
  const rootRef = useRef<HTMLDivElement>(null)
  const activatedButton = useRef<HTMLButtonElement | null>(null)
  const selected = options.find((option) => option.value === value)
  const selectedIndex = options.findIndex((option) => option.value === value)
  const transition = {
    duration: reducedMotion ? 0 : 0.38,
    ease: [0.22, 1, 0.36, 1] as const,
  }

  useEffect(() => {
    // Restore keyboard focus when the activated alternative leaves the DOM.
    if (
      layout === 'focus' &&
      activatedButton.current &&
      (document.activeElement === activatedButton.current ||
        !activatedButton.current.isConnected)
    ) {
      rootRef.current
        ?.querySelector<HTMLButtonElement>(
          '[data-slot="expressive-choice-current"]',
        )
        ?.focus({ preventScroll: true })
    }
    activatedButton.current = null
  }, [value, layout])

  return (
    <div
      ref={rootRef}
      role="group"
      aria-label={label}
      data-slot="expressive-choice"
      data-layout={layout}
      className={cn(
        '@container-size min-h-0 w-full flex-1 gap-2 [container-name:expressive-choice]',
        layout === 'focus'
          ? 'grid grid-rows-[minmax(0,1fr)_2.5rem]'
          : 'flex flex-col',
      )}
    >
      {layout === 'focus' && (
        <Button
          asChild
          variant="basic"
          disableRipple
          disabled={disabled || !selected}
          aria-label={selected?.label}
          aria-pressed={Boolean(selected)}
          className="bg-primary-container text-on-primary-container hover:bg-primary-container dark:bg-primary-container dark:text-on-primary-container dark:hover:bg-primary-container focus-visible:ring-primary relative flex h-auto min-h-0 min-w-0 flex-[1.7] items-center justify-start gap-4 overflow-hidden rounded-3xl px-4 py-2 shadow-none focus-visible:ring-2 [@container_expressive-choice_(max-height:110px)]:gap-2.5 [@container_expressive-choice_(max-height:110px)]:px-3 [@container_expressive-choice_(max-height:110px)]:py-1"
        >
          <motion.button
            type="button"
            whileTap={reducedMotion ? undefined : { scale: 0.97 }}
            data-slot="expressive-choice-current"
          >
            <motion.span
              data-slot="expressive-choice-current-shape"
              className="[container-type:size] relative grid aspect-square h-full max-h-16 shrink-0 place-items-center"
              initial={false}
              animate={{
                scale: reducedMotion
                  ? 1
                  : selectedIndex % 2
                    ? [0.94, 1.04, 1]
                    : [0.96, 1.025, 1],
              }}
              transition={transition}
            >
              <MaterialShape
                depth={6 + Math.max(0, selectedIndex) * 2}
                className="bg-primary absolute inset-0"
              />
              <AnimatePresence mode="popLayout" initial={false}>
                <motion.span
                  key={value}
                  className="text-on-primary relative flex w-[clamp(1rem,44cqi,1.75rem)] [&_svg]:size-full"
                  initial={{
                    opacity: 0,
                    scale: reducedMotion ? 1 : 0.7,
                    y: reducedMotion ? 0 : 8,
                  }}
                  animate={{ opacity: 1, scale: 1, y: 0 }}
                  exit={{
                    opacity: 0,
                    scale: reducedMotion ? 1 : 0.7,
                    y: reducedMotion ? 0 : -8,
                  }}
                  transition={transition}
                >
                  {selected?.icon}
                </motion.span>
              </AnimatePresence>
            </motion.span>
            <div
              className="relative min-w-0 flex-1 text-left"
              aria-live="polite"
            >
              <AnimatePresence mode="popLayout" initial={false}>
                <motion.div
                  key={value}
                  data-slot="expressive-choice-current-copy"
                  initial={{ opacity: 0, x: reducedMotion ? 0 : 14 }}
                  animate={{ opacity: 1, x: 0 }}
                  exit={{ opacity: 0, x: reducedMotion ? 0 : -14 }}
                  transition={transition}
                >
                  <div className="truncate text-lg font-medium [@container_expressive-choice_(max-height:110px)]:text-sm [@container_expressive-choice_(max-height:110px)]:leading-5">
                    {selected?.label ?? '—'}
                  </div>
                  {selected?.description && (
                    <div
                      className="mt-1 line-clamp-2 text-xs font-normal opacity-80 [@container_expressive-choice_(max-height:110px)]:hidden"
                      data-slot="expressive-choice-description"
                    >
                      {selected.description}
                    </div>
                  )}
                </motion.div>
              </AnimatePresence>
            </div>
          </motion.button>
        </Button>
      )}
      <div
        data-slot="expressive-choice-options"
        className="flex min-h-0 flex-1 items-stretch gap-2"
      >
        <AnimatePresence initial={false} mode="popLayout">
          {options
            .filter((option) => layout !== 'focus' || option.value !== value)
            .map((option) => {
              const active = option.value === value

              return (
                <motion.div
                  layout={reducedMotion ? false : 'position'}
                  key={option.value}
                  data-value={option.value}
                  data-active={active}
                  className="relative min-w-0"
                  initial={{ opacity: 0 }}
                  animate={{
                    opacity: 1,
                    flexGrow: layout === 'flex' && active ? 1.2 : 1,
                    flexBasis: 0,
                  }}
                  exit={{ opacity: 0, scale: reducedMotion ? 1 : 0.94 }}
                  transition={transition}
                >
                  <Button
                    asChild
                    variant="basic"
                    disableRipple={layout === 'flex'}
                    disabled={disabled}
                    aria-pressed={active}
                    onClick={(event) => {
                      if (document.activeElement === event.currentTarget)
                        activatedButton.current = event.currentTarget
                      onChange?.(option.value)
                    }}
                    className={cn(
                      'group focus-visible:ring-primary relative isolate flex h-full min-h-10 w-full min-w-0 items-center justify-center gap-2 overflow-hidden rounded-2xl px-2 py-1 shadow-none focus-visible:ring-2 [@container_expressive-choice_(max-height:110px)]:gap-1',
                      layout === 'flex'
                        ? 'flex-col transition-colors duration-200'
                        : 'bg-surface-variant/40 text-on-surface-variant dark:bg-surface-variant/40 dark:text-on-surface-variant dark:hover:bg-surface-variant/60',
                      layout === 'flex' &&
                        (active
                          ? 'bg-primary-container text-on-primary-container hover:bg-primary-container dark:bg-primary-container dark:text-on-primary-container dark:hover:bg-primary-container'
                          : 'bg-surface-variant/35 text-on-surface-variant hover:bg-surface-variant/50 dark:bg-surface-variant/35 dark:text-on-surface-variant dark:hover:bg-surface-variant/50'),
                    )}
                  >
                    <motion.button
                      type="button"
                      whileTap={reducedMotion ? undefined : { scale: 0.95 }}
                    >
                      <motion.span
                        className={cn(
                          'relative isolate grid shrink-0 place-items-center',
                          layout === 'flex'
                            ? 'size-11 [&_svg]:relative [&_svg]:size-6 [@container_expressive-choice_(max-height:110px)]:size-8 [@container_expressive-choice_(max-height:110px)]:[&_svg]:size-5'
                            : '[&_svg]:size-4',
                        )}
                        initial={false}
                        animate={{
                          y: layout === 'flex' && active ? -2 : 0,
                          scale: active ? 1.08 : 1,
                        }}
                        transition={transition}
                      >
                        {layout === 'flex' && (
                          <MaterialShape
                            className={cn(
                              'bg-primary absolute inset-0 -z-10 transition-opacity duration-200',
                              active ? 'opacity-100' : 'opacity-0',
                            )}
                            active={active}
                          />
                        )}
                        <span
                          className={cn(
                            'relative flex',
                            layout === 'flex' && active && 'text-on-primary',
                          )}
                        >
                          {option.icon}
                        </span>
                      </motion.span>
                      <span className="relative max-w-full truncate text-sm">
                        {option.label}
                      </span>
                      {layout === 'flex' && (
                        <motion.span
                          className="absolute top-2 right-2"
                          initial={false}
                          animate={{
                            opacity: active ? 1 : 0,
                            scale: active ? 1 : 0.7,
                          }}
                          transition={transition}
                          aria-hidden
                        >
                          <CheckRounded className="size-3" />
                        </motion.span>
                      )}
                    </motion.button>
                  </Button>
                </motion.div>
              )
            })}
        </AnimatePresence>
      </div>
      {layout === 'flex' && selected?.description && (
        <div
          className="text-on-surface-variant min-h-4 truncate text-xs [@container_expressive-choice_(max-height:110px)]:hidden"
          data-slot="expressive-choice-description"
          aria-live="polite"
        >
          {selected.description}
        </div>
      )}
    </div>
  )
}
