import { Checkbox } from '@nyanpasu/ui/checkbox'
import type { Scope } from '@nyanpasu/rpc/types'
import { scopeLabel } from './scope-labels'

/** One labelled checkbox per offered scope, in the order offered. */
export default function ScopeCheckboxes({
  scopes,
  value,
  disabled,
  onChange,
}: {
  scopes: readonly Scope[]
  value: readonly Scope[]
  disabled?: boolean
  onChange: (next: Scope[]) => void
}) {
  return (
    <div className="flex flex-col gap-2" data-slot="scope-checkboxes">
      {scopes.map((scope) => (
        <label
          key={scope}
          className="flex cursor-pointer items-center gap-3 text-sm"
          data-slot="scope-checkbox"
        >
          <Checkbox
            checked={value.includes(scope)}
            disabled={disabled}
            onCheckedChange={(checked) =>
              onChange(
                checked
                  ? scopes.filter((s) => s === scope || value.includes(s))
                  : value.filter((s) => s !== scope),
              )
            }
          />

          {scopeLabel(scope)}
        </label>
      ))}
    </div>
  )
}
