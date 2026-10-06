import CloseRounded from '~icons/material-symbols/close-rounded'
import SearchRounded from '~icons/material-symbols/search-rounded'
import {
  ComponentProps,
  KeyboardEvent,
  useImperativeHandle,
  useRef,
} from 'react'
import { cn } from '@nyanpasu/utils'

export type SearchFieldProps = Omit<
  ComponentProps<'input'>,
  'value' | 'onChange' | 'type'
> & {
  value: string
  onValueChange: (value: string) => void
  /** Accessible name of the clear button. */
  clearLabel: string
}

export const SearchField = ({
  value,
  onValueChange,
  clearLabel,
  className,
  onKeyDown,
  ref,
  placeholder,
  disabled,
  readOnly,
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  ...props
}: SearchFieldProps) => {
  const inputRef = useRef<HTMLInputElement>(null)

  useImperativeHandle(ref, () => inputRef.current as HTMLInputElement)

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    onKeyDown?.(event)

    if (event.key === 'Escape' && value && !readOnly) {
      onValueChange('')
    }
  }

  const handleClear = () => {
    onValueChange('')
    inputRef.current?.focus()
  }

  return (
    <div
      data-slot="search-field"
      className={cn(
        'relative flex h-10 min-w-0 items-center rounded-full',
        'bg-surface-variant dark:bg-surface-variant/30 text-on-surface',
        'focus-within:outline-primary focus-within:outline-2',
        className,
      )}
    >
      <SearchRounded
        aria-hidden
        className="text-on-surface-variant ml-3 size-5 shrink-0"
      />

      <input
        ref={inputRef}
        type="search"
        data-slot="search-field-input"
        className={cn(
          'h-full min-w-0 flex-1 bg-transparent px-3 text-sm outline-none',
          'placeholder:text-on-surface-variant',
          '[&::-webkit-search-cancel-button]:appearance-none',
        )}
        autoComplete="off"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        readOnly={readOnly}
        aria-label={ariaLabel ?? (ariaLabelledBy ? undefined : placeholder)}
        aria-labelledby={ariaLabelledBy}
        onChange={(event) => onValueChange(event.target.value)}
        onKeyDown={handleKeyDown}
        {...props}
      />

      {value && !disabled && !readOnly && (
        <button
          type="button"
          data-slot="search-field-clear"
          aria-label={clearLabel}
          className={cn(
            'mr-1 grid size-8 shrink-0 cursor-pointer place-content-center rounded-full',
            'text-on-surface-variant hover:bg-on-surface/8',
            'focus-visible:outline-primary outline-none focus-visible:outline-2',
          )}
          onClick={handleClear}
        >
          <CloseRounded aria-hidden className="size-5" />
        </button>
      )}
    </div>
  )
}
