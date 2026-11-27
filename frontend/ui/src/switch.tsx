import CheckRounded from '~icons/material-symbols/check-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { motion, useReducedMotion } from 'motion/react'
import { Switch as SwitchPrimitives } from 'radix-ui'
import React, { ComponentProps, useState } from 'react'
import { chains, cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { CircularProgress } from './progress'

const SWITCH_THUMB_SPRING = {
  type: 'spring' as const,
  stiffness: 600,
  damping: 32,
  mass: 1,
}

export const Switch = ({
  className,
  children,
  loading,
  checked,
  defaultChecked,
  onCheckedChange,
  disabled,
  onPointerDown,
  onPointerUp,
  onPointerCancel,
  onPointerLeave,
  ...props
}: React.ComponentProps<typeof SwitchPrimitives.Root> & {
  loading?: boolean
}) => {
  const reducedMotion = useReducedMotion()
  const [isPressed, setIsPressed] = useState(false)
  const [isChecked = false, setIsChecked] = useControllableState({
    prop: checked,
    defaultProp: defaultChecked ?? false,
    onChange: onCheckedChange,
  })

  const loadingPressed = Boolean(loading && isPressed && !disabled)
  // M3 / Android 17 switch with icons in both states uses a 24px resting thumb (28px when pressed while loading).
  const thumbSize = loadingPressed ? 28 : 24
  // Symmetric 4px outer inset inside the 52x32px track (border-2 + 2px inner gap): center locked at 14px (off) and 34px (on).
  const thumbX = isChecked ? (loadingPressed ? 20 : 22) : loadingPressed ? 0 : 2

  return (
    <SwitchPrimitives.Root
      data-slot="switch"
      checked={isChecked}
      onCheckedChange={setIsChecked}
      disabled={disabled}
      onPointerDown={chains(onPointerDown, () => {
        if (loading && !disabled) setIsPressed(true)
      })}
      onPointerUp={chains(onPointerUp, () => setIsPressed(false))}
      onPointerCancel={chains(onPointerCancel, () => setIsPressed(false))}
      onPointerLeave={chains(onPointerLeave, () => setIsPressed(false))}
      className={cn(
        'peer',
        'inline-flex h-8 w-13 shrink-0 cursor-pointer items-center rounded-full',
        'focus-visible:ring-primary focus-visible:ring-offset-surface focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none',
        'disabled:cursor-not-allowed disabled:opacity-38',
        'border-2 transition-colors duration-150 motion-reduce:transition-none',
        'data-[state=unchecked]:border-outline data-[state=checked]:border-transparent',
        'data-[state=checked]:bg-primary data-[state=unchecked]:bg-surface-container-highest',
        className,
      )}
      {...props}
    >
      <SwitchPrimitives.Thumb asChild>
        <motion.span
          data-slot="switch-thumb"
          initial={false}
          animate={{ x: thumbX, width: thumbSize, height: thumbSize }}
          transition={reducedMotion ? { duration: 0 } : SWITCH_THUMB_SPRING}
          className={cn(
            'pointer-events-none relative grid shrink-0 place-items-center',
            'rounded-full ring-0 transition-colors duration-150 motion-reduce:transition-none',
            isChecked ? 'bg-on-primary' : 'bg-outline',
          )}
        >
          {loading ? (
            <span
              className="grid size-full place-items-center"
              data-slot="switch-thumb-loading"
            >
              <CircularProgress
                className={cn(
                  'size-4',
                  isChecked ? 'text-primary' : 'text-surface-container-highest',
                )}
                data-slot="switch-thumb-loading-circular-progress"
                indeterminate
              />
            </span>
          ) : (
            <>
              <motion.span
                aria-hidden
                data-slot="switch-thumb-check"
                initial={false}
                animate={{
                  opacity: isChecked ? 1 : 0,
                  scale: isChecked ? 1 : 0.6,
                }}
                transition={
                  reducedMotion
                    ? { duration: 0 }
                    : { duration: 0.14, ease: 'easeOut' }
                }
                className="text-primary absolute inset-0 grid place-items-center"
              >
                <CheckRounded className="size-4" />
              </motion.span>
              <motion.span
                aria-hidden
                data-slot="switch-thumb-close"
                initial={false}
                animate={{
                  opacity: isChecked ? 0 : 1,
                  scale: isChecked ? 0.6 : 1,
                }}
                transition={
                  reducedMotion
                    ? { duration: 0 }
                    : { duration: 0.14, ease: 'easeOut' }
                }
                className="text-surface-container-highest absolute inset-0 grid place-items-center"
              >
                <CloseRounded className="size-4" />
              </motion.span>
            </>
          )}
        </motion.span>
      </SwitchPrimitives.Thumb>

      {children}
    </SwitchPrimitives.Root>
  )
}

export function SwitchItem({
  children,
  className,
  ...props
}: ComponentProps<typeof Switch>) {
  return (
    <div
      data-slot="switch-item"
      className={cn(
        'flex min-h-16 w-full items-center justify-between gap-3',
        'bg-surface-container-lowest dark:bg-surface-container',
        'rounded-large',
        'px-4 py-3',
        className,
      )}
    >
      {children}

      <Switch {...props} />
    </div>
  )
}
