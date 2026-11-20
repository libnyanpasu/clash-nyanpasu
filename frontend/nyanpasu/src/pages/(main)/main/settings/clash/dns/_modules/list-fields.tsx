import AddRounded from '~icons/material-symbols/add-rounded'
import DeleteRounded from '~icons/material-symbols/delete-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Input } from '@nyanpasu/ui/input'
import { m } from '@/paraglide/messages'
import type { OneOrMany } from '@nyanpasu/rpc/types'
import {
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../../../_modules/settings-card'

/**
 * An ordered list of strings, one input per entry. Order matters for upstream
 * servers, so entries are edited in place rather than sorted.
 */
export function StringListField({
  label,
  description,
  values,
  placeholder,
  onChange,
}: {
  label: string
  description?: string
  values: string[]
  placeholder?: string
  onChange: (values: string[]) => void
}) {
  const setAt = (index: number, value: string) =>
    onChange(values.map((item, current) => (current === index ? value : item)))

  return (
    <div className="flex flex-col gap-2" data-slot="string-list-field">
      <ItemLabel>
        <ItemLabelText>{label}</ItemLabelText>

        {description && (
          <ItemLabelDescription>{description}</ItemLabelDescription>
        )}
      </ItemLabel>

      {values.map((value, index) => (
        <div
          className="flex items-center gap-2"
          data-slot="string-list-field-row"
          // Entries may repeat while being typed, so the position is the
          // identity.
          key={index}
        >
          <Input
            className="flex-1"
            value={value}
            placeholder={placeholder}
            onChange={(event) => setAt(index, event.target.value)}
          />

          <Button
            icon
            aria-label={m.settings_clash_dns_remove_entry()}
            onClick={() =>
              onChange(values.filter((_, current) => current !== index))
            }
          >
            <DeleteRounded className="size-5" />
          </Button>
        </div>
      ))}

      <Button
        className="flex items-center gap-1 self-start px-4"
        variant="stroked"
        onClick={() => onChange([...values, ''])}
      >
        <AddRounded className="size-5" />
        <span>{m.settings_clash_dns_add_entry()}</span>
      </Button>
    </div>
  )
}

/** `a, b` as typed, kept as one entry or a list of them. */
export const parseOneOrMany = (text: string): OneOrMany => {
  const parts = text
    .split(',')
    .map((part) => part.trim())
    .filter(Boolean)

  return parts.length === 1 ? parts[0] : parts
}

export const formatOneOrMany = (value: OneOrMany | undefined): string =>
  Array.isArray(value) ? value.join(', ') : (value ?? '')

type MappingRow = [string, OneOrMany]

/**
 * One `key: value` row. Both inputs keep their text while focused and commit
 * on blur, so a half-typed key cannot collide with another row and a trailing
 * comma is not parsed away mid-entry.
 */
function MappingRowEditor({
  row,
  keyPlaceholder,
  valuePlaceholder,
  onCommit,
  onRemove,
}: {
  row: MappingRow
  keyPlaceholder: string
  valuePlaceholder: string
  onCommit: (row: MappingRow) => void
  onRemove: () => void
}) {
  const [key, setKey] = useState(row[0])

  const [text, setText] = useState(formatOneOrMany(row[1]))

  const commit = () => onCommit([key.trim(), parseOneOrMany(text)])

  return (
    <div className="flex items-center gap-2" data-slot="mapping-field-row">
      <Input
        className="flex-1"
        value={key}
        placeholder={keyPlaceholder}
        onChange={(event) => setKey(event.target.value)}
        onBlur={commit}
      />

      <Input
        className="flex-1"
        value={text}
        placeholder={valuePlaceholder}
        onChange={(event) => setText(event.target.value)}
        onBlur={commit}
      />

      <Button
        icon
        aria-label={m.settings_clash_dns_remove_entry()}
        onClick={onRemove}
      >
        <DeleteRounded className="size-5" />
      </Button>
    </div>
  )
}

/**
 * An ordered `key: value` mapping whose values are one entry or a
 * comma-separated list, such as `nameserver-policy` and `hosts`.
 */
export function MappingField({
  label,
  description,
  entries,
  keyPlaceholder,
  valuePlaceholder,
  onChange,
}: {
  label: string
  description?: string
  entries: Record<string, OneOrMany>
  keyPlaceholder: string
  valuePlaceholder: string
  onChange: (entries: Record<string, OneOrMany>) => void
}) {
  const rows = Object.entries(entries) as MappingRow[]

  const commit = (next: MappingRow[]) => onChange(Object.fromEntries(next))

  return (
    <div className="flex flex-col gap-2" data-slot="mapping-field">
      <ItemLabel>
        <ItemLabelText>{label}</ItemLabelText>

        {description && (
          <ItemLabelDescription>{description}</ItemLabelDescription>
        )}
      </ItemLabel>

      {rows.map((row, index) => (
        <MappingRowEditor
          // Remount when the saved row changes underneath, such as after a
          // YAML edit, so the inputs show it.
          key={`${index}:${row[0]}:${formatOneOrMany(row[1])}`}
          row={row}
          keyPlaceholder={keyPlaceholder}
          valuePlaceholder={valuePlaceholder}
          onCommit={(next) =>
            commit(
              rows.map((current, position) =>
                position === index ? next : current,
              ),
            )
          }
          onRemove={() =>
            commit(rows.filter((_, position) => position !== index))
          }
        />
      ))}

      <Button
        className="flex items-center gap-1 self-start px-4"
        variant="stroked"
        // A mapping cannot hold two empty keys, so each new row starts with a
        // name the user replaces.
        onClick={() => commit([...rows, [`new-${rows.length + 1}`, '']])}
      >
        <AddRounded className="size-5" />
        <span>{m.settings_clash_dns_add_entry()}</span>
      </Button>
    </div>
  )
}
