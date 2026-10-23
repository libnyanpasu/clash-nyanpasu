import { m } from '@/paraglide/messages'
import type { BuiltinStepKind } from '@nyanpasu/rpc/types'

/**
 * Labels of the built-in steps Nyanpasu runs, in pipeline order. The map is
 * exhaustive, so a new step kind fails to compile until it has a label. The
 * core controller step is appended by inspection and is not a setting.
 */
export const BUILTIN_STEP_LABELS = {
  field_filter: () => m.inspect_filter(),
  keep_unmanaged: () => m.inspect_keep_unmanaged(),
  managed_settings: () => m.inspect_forced_settings(),
  defaults: () => m.inspect_defaults(),
  include_all_expansion: () => m.inspect_include_all(),
} satisfies Record<Exclude<BuiltinStepKind, 'core_controller'>, () => string>
