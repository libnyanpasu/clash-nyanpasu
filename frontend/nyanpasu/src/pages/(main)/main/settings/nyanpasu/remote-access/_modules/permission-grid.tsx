import { cva } from 'class-variance-authority'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import type { HeldPermissions } from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'
import { GRANTABLE_RESOURCES } from '@nyanpasu/rpc'
import type { Access } from '@nyanpasu/rpc/types'
import { ACCESS_LABELS, RESOURCE_LABELS } from './permission-labels'

const LEVELS = ['none', 'read', 'write'] as const

type Level = (typeof LEVELS)[number]

const LEVEL_RANK = { none: 0, read: 1, write: 2 } satisfies Record<
  Level,
  number
>

function levelLabel(level: Level) {
  return level === 'none'
    ? m.settings_remote_access_access_none()
    : ACCESS_LABELS[level]()
}

const levelChipVariants = cva(
  'w-20 shrink-0 rounded-full px-3 py-1 text-center text-xs font-medium',
  {
    variants: {
      level: {
        none: 'bg-surface-variant/40 text-on-surface-variant',
        read: 'bg-secondary-container text-on-secondary-container',
        write: 'bg-primary-container text-on-primary-container',
      },
    },
  },
)

/**
 * One row per grantable resource with a none / read / write choice. Levels above `limit` cannot
 * be chosen; `write` includes `read`. Without `onChange` the grid only shows `value`, as a role
 * grants it.
 */
export default function PermissionGrid({
  value,
  limit,
  disabled,
  onChange,
}: {
  value: HeldPermissions
  /** The highest level each resource may take; a resource that is absent takes none. */
  limit?: HeldPermissions
  disabled?: boolean
  onChange?: (next: HeldPermissions) => void
}) {
  return (
    <div className="flex flex-col gap-2" data-slot="permission-grid">
      {GRANTABLE_RESOURCES.map((resource) => {
        const highest: Level = limit ? (limit[resource] ?? 'none') : 'write'
        const level: Level = value[resource] ?? 'none'

        return (
          <div
            key={resource}
            className="flex items-center justify-between gap-3 text-sm"
            data-slot="permission-grid-row"
          >
            <span>{RESOURCE_LABELS[resource]()}</span>

            {onChange ? (
              <SegmentedButton
                size="sm"
                className="w-56 shrink-0"
                aria-label={RESOURCE_LABELS[resource]()}
                disabled={disabled}
                value={level}
                onValueChange={(next) => {
                  if (!next) {
                    return
                  }
                  const { [resource]: _previous, ...rest } = value
                  onChange(
                    next === 'none'
                      ? rest
                      : { ...rest, [resource]: next as Access },
                  )
                }}
              >
                {LEVELS.map((item) => (
                  <SegmentedButtonItem
                    key={item}
                    value={item}
                    disabled={LEVEL_RANK[item] > LEVEL_RANK[highest]}
                  >
                    {levelLabel(item)}
                  </SegmentedButtonItem>
                ))}
              </SegmentedButton>
            ) : (
              <span
                className={levelChipVariants({ level })}
                data-slot="permission-grid-level"
                aria-label={`${RESOURCE_LABELS[resource]()}: ${levelLabel(level)}`}
              >
                {levelLabel(level)}
              </span>
            )}
          </div>
        )
      })}
    </div>
  )
}
