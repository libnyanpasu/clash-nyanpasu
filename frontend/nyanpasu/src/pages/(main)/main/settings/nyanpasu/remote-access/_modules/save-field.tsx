import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Input } from '@nyanpasu/ui/input'
import { m } from '@/paraglide/messages'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../../../_modules/settings-card'

/**
 * A text setting that is saved with its own button. `parse` returns the value to save, `null`
 * for an empty field that clears the setting, or `undefined` for text that is not valid.
 */
export default function SaveField<T>({
  slot,
  title,
  description,
  invalidMessage,
  inputMode,
  committed,
  parse,
  disabled,
  onSave,
}: {
  slot: string
  title: string
  description: string
  invalidMessage: string
  inputMode?: 'numeric'
  committed: T | null
  parse: (text: string) => T | null | undefined
  disabled: boolean
  onSave: (value: T | null) => void
}) {
  const [text, setText] = useState(committed == null ? '' : String(committed))

  const parsed = parse(text)
  const invalid = parsed === undefined
  const unchanged = parsed === committed

  return (
    <div className="flex flex-col gap-2" data-slot={slot}>
      <ItemLabel>
        <ItemLabelText>{title}</ItemLabelText>

        <ItemLabelDescription>{description}</ItemLabelDescription>
      </ItemLabel>

      <form
        className="flex items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault()
          if (!invalid && !unchanged) {
            onSave(parsed)
          }
        }}
      >
        <Input
          className="w-full"
          label={title}
          aria-label={title}
          inputMode={inputMode}
          value={text}
          disabled={disabled}
          onChange={(event) => setText(event.target.value)}
        />

        <Button
          type="submit"
          variant="flat"
          disabled={disabled || invalid || unchanged}
        >
          {m.common_save()}
        </Button>
      </form>

      {invalid && (
        <p role="alert" className="text-error text-sm">
          {invalidMessage}
        </p>
      )}
    </div>
  )
}
