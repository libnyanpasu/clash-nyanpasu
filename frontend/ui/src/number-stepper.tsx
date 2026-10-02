import { useEffect, useId, useRef, useState } from 'react'
import { cn } from '@nyanpasu/utils'

export function NumberStepper({
  label,
  value,
  min,
  max,
  decrementLabel,
  incrementLabel,
  disabled = false,
  onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  decrementLabel: string
  incrementLabel: string
  disabled?: boolean
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

  return (
    <div className="space-y-2" data-slot="number-stepper">
      <label htmlFor={inputId} className="text-on-surface-variant text-xs">
        {label}
      </label>
      <div className="border-outline-variant/60 bg-surface-variant/20 flex h-9 items-center overflow-hidden rounded-xl border">
        <button
          type="button"
          aria-label={decrementLabel}
          className="hover:bg-surface-variant focus-visible:ring-primary h-full w-9 focus-visible:z-10 focus-visible:ring-2 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-40"
          disabled={disabled || Number(draft) <= min}
          onPointerDown={(event) => event.preventDefault()}
          onClick={() => stepBy(-1)}
        >
          −
        </button>
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
            'text-on-surface focus-visible:ring-primary min-w-0 flex-1 bg-transparent text-center text-sm outline-none focus-visible:ring-2 focus-visible:ring-inset',
            disabled && 'opacity-40',
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
        <button
          type="button"
          aria-label={incrementLabel}
          className="hover:bg-surface-variant focus-visible:ring-primary h-full w-9 focus-visible:z-10 focus-visible:ring-2 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-40"
          disabled={disabled || Number(draft) >= max}
          onPointerDown={(event) => event.preventDefault()}
          onClick={() => stepBy(1)}
        >
          +
        </button>
      </div>
    </div>
  )
}
