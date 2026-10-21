import { m } from '@/paraglide/messages'
import { isDeprecatedClashCore } from '@nyanpasu/query'
import { ClashCore } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'

export default function DeprecatedCoreBadge({
  core,
}: {
  core?: ClashCore | null
}) {
  if (!isDeprecatedClashCore(core)) {
    return null
  }

  return (
    <span
      className={cn(
        'inline-flex h-5 shrink-0 items-center rounded-full px-2 text-xs font-medium',
        'bg-error-container text-on-error-container',
      )}
      data-slot="deprecated-core-badge"
    >
      {m.settings_clash_core_deprecated()}
    </span>
  )
}
