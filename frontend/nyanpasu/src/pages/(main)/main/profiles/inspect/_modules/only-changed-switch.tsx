import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

/** The "only steps that changed something" filter, below the list or graph. */
export default function OnlyChangedSwitch({
  checked,
  onCheckedChange,
  className,
}: {
  checked: boolean
  onCheckedChange: (checked: boolean) => void
  className?: string
}) {
  return (
    <label
      data-slot="inspect-only-changed"
      className={cn(
        'text-on-surface-variant flex shrink-0 cursor-pointer items-center justify-between gap-3 px-2 text-[13px]',
        className,
      )}
    >
      {m.inspect_only_changed()}

      <Switch checked={checked} onCheckedChange={onCheckedChange} />
    </label>
  )
}
