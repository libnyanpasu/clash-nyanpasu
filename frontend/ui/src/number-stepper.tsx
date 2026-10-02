import AddRounded from '~icons/material-symbols/add-rounded'
import RemoveRounded from '~icons/material-symbols/remove-rounded'
import { useEffect, useId, useRef, useState } from 'react'
import { cn } from '@nyanpasu/utils'
import { Button } from './button'

export type NumberStepperVariant = 'outlined' | 'filled' | 'tonal'

const buttonStyles: Record<NumberStepperVariant, string> = {
  outlined:
    'border border-outline bg-transparent text-on-surface-variant hover:bg-on-surface/8 dark:bg-transparent dark:text-on-surface-variant dark:hover:bg-on-surface/8 disabled:border-on-surface/12 disabled:bg-transparent disabled:hover:bg-transparent dark:disabled:bg-transparent dark:disabled:hover:bg-transparent',
  filled:
    'bg-primary text-on-primary hover:bg-primary hover:brightness-95 dark:bg-primary dark:text-on-primary dark:hover:bg-primary disabled:bg-on-surface/12 disabled:hover:bg-on-surface/12 disabled:hover:brightness-100 dark:disabled:bg-on-surface/12 dark:disabled:hover:bg-on-surface/12',
  tonal:
    'bg-secondary-container text-on-secondary-container hover:bg-secondary-container hover:brightness-95 dark:bg-secondary-container dark:text-on-secondary-container dark:hover:bg-secondary-container disabled:bg-on-surface/12 disabled:hover:bg-on-surface/12 disabled:hover:brightness-100 dark:disabled:bg-on-surface/12 dark:disabled:hover:bg-on-surface/12',
}

const inputStyles: Record<NumberStepperVariant, string> = {
  outlined: 'border-outline bg-transparent',
  filled: 'border-transparent bg-surface-variant/30',
  tonal: 'border-transparent bg-secondary-container/40',
}

export function NumberStepper({
  label,
  value,
  min,
  max,
  decrementLabel,
  incrementLabel,
  disabled = false,
  variant = 'outlined',
  onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  decrementLabel: string
  incrementLabel: string
  disabled?: boolean
  variant?: NumberStepperVariant
  onChange: (value: number) => void
}) {
  const inputId = useId()
  const draftRef = useRef(String(value))
  const lastSubmittedRef = useRef<number | null>(null)
  const [draft, setDraft] = useState(String(value))
  const focusedRef = useRef(false)

  useEffect(() => {
    if (!focusedRef.current) {
      draftRef.current = String(value)
      setDraft(String(value))
    }
  }, [value])

  const setDraftValue = (next: string) => {
    draftRef.current = next
    setDraft(next)
  }

  const commit = () => {
    const currentDraft = draftRef.current
    if (!/^-?\d+$/.test(currentDraft)) {
      lastSubmittedRef.current = null
      setDraftValue(String(value))
      return
    }

    const next = Math.min(max, Math.max(min, Number(currentDraft)))
    setDraftValue(String(next))
    if (next !== value && next !== lastSubmittedRef.current) {
      lastSubmittedRef.current = next
      onChange(next)
    }
  }

  const submit = (next: number) => {
    const bounded = Math.min(max, Math.max(min, next))
    setDraftValue(String(bounded))
    lastSubmittedRef.current = bounded
    if (bounded !== value) onChange(bounded)
  }

  const stepBy = (delta: number) => {
    const currentDraft = draftRef.current
    const parsed = /^-?\d+$/.test(currentDraft) ? Number(currentDraft) : value
    const boundedDraft = Math.min(max, Math.max(min, parsed))
    submit(boundedDraft + delta)
  }

  const buttonClassName = cn(
    'size-10 shrink-0 focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface disabled:text-on-surface/38 dark:disabled:text-on-surface/38',
    buttonStyles[variant],
  )

  return (
    <div
      className="space-y-2"
      data-slot="number-stepper"
      data-variant={variant}
    >
      <label
        htmlFor={inputId}
        className={cn(
          'block text-xs',
          disabled ? 'text-on-surface/38' : 'text-on-surface-variant',
        )}
      >
        {label}
      </label>
      <div
        data-slot="number-stepper-field"
        className="flex h-10 items-center gap-2"
      >
        <Button
          type="button"
          variant="basic"
          icon
          aria-label={decrementLabel}
          className={buttonClassName}
          disabled={disabled || Number(draft) <= min}
          onPointerDown={(event) => event.preventDefault()}
          onClick={() => stepBy(-1)}
        >
          <RemoveRounded className="size-6" />
        </Button>
        <input
          id={inputId}
          type="text"
          inputMode={min < 0 ? 'decimal' : 'numeric'}
          role="spinbutton"
          aria-valuemin={min}
          aria-valuemax={max}
          aria-valuenow={
            /^-?\d+$/.test(draft)
              ? Math.min(max, Math.max(min, Number(draft)))
              : value
          }
          value={draft}
          disabled={disabled}
          className={cn(
            'text-on-surface focus:border-primary focus:ring-primary h-full min-w-0 flex-1 rounded-full border px-3 text-center text-base tabular-nums transition-colors outline-none focus:ring-1',
            inputStyles[variant],
            disabled &&
              'border-on-surface/12 bg-on-surface/4 text-on-surface/38',
          )}
          onChange={(event) => {
            const nextDraft = event.currentTarget.value
            lastSubmittedRef.current = null
            setDraftValue(nextDraft)
          }}
          onFocus={() => {
            focusedRef.current = true
          }}
          onBlur={() => {
            focusedRef.current = false
            commit()
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              commit()
            } else if (event.key === 'ArrowUp') {
              event.preventDefault()
              stepBy(1)
            } else if (event.key === 'ArrowDown') {
              event.preventDefault()
              stepBy(-1)
            } else if (event.key === 'Home') {
              event.preventDefault()
              submit(min)
            } else if (event.key === 'End') {
              event.preventDefault()
              submit(max)
            }
          }}
        />
        <Button
          type="button"
          variant="basic"
          icon
          aria-label={incrementLabel}
          className={buttonClassName}
          disabled={disabled || Number(draft) >= max}
          onPointerDown={(event) => event.preventDefault()}
          onClick={() => stepBy(1)}
        >
          <AddRounded className="size-6" />
        </Button>
      </div>
    </div>
  )
}
