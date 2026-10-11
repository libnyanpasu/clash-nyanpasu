import LayersOutlineRounded from '~icons/material-symbols/layers-outline-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import type { ComponentProps } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@nyanpasu/ui/popover'
import { m } from '@/paraglide/messages'
import {
  getProfileSource,
  getRemoteSource,
  isConfigItem,
  scopedTransformsOf,
} from '@nyanpasu/query'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'

export const ProfileCardChip = ({
  className,
  ...props
}: ComponentProps<'span'>) => (
  <span
    className={cn(
      'inline-flex items-center gap-1 rounded-full px-3 py-1 text-xs font-bold whitespace-nowrap',
      'bg-surface-variant text-on-surface-variant',
      className,
    )}
    {...props}
  />
)

const sourceLabelOf = (profile: ProfileItem_Serialize) => {
  const source = getProfileSource(profile)

  if (!source) return m.profile_kind_composition()
  if (source.type === 'remote') return m.profile_source_remote()

  return source.binding.type === 'external'
    ? m.profile_source_external()
    : m.profile_source_local()
}

const transformLabelOf = (profile: ProfileItem_Serialize) => {
  if (profile.type !== 'transform') return undefined
  if (profile.transform.type === 'overlay') return 'YAML'

  return profile.transform.runtime === 'javascript' ? 'JavaScript' : 'Lua'
}

const updateIntervalLabel = (minutes: number) => {
  if (minutes % 1440 === 0) return `${minutes / 1440}d`
  if (minutes % 60 === 0) return `${minutes / 60}h`

  return `${minutes}min`
}

export default function ProfileCardFeatures({
  profile,
  profiles,
  isGlobal,
}: {
  profile: ProfileItem_Serialize
  profiles: ProfileItem_Serialize[]
  isGlobal: boolean
}) {
  const remote = getRemoteSource(profile)
  const transforms = scopedTransformsOf(profile)
  const transformLabel = transformLabelOf(profile)
  const sourceCount =
    isConfigItem(profile) && profile.config.type === 'composition'
      ? new Set([
          ...(profile.config.base ? [profile.config.base] : []),
          ...(profile.config.extend_proxies_from ?? []),
        ]).size
      : 0

  return (
    <div
      className="z-10 flex flex-wrap items-center gap-1"
      data-slot="profile-card-type"
    >
      <ProfileCardChip className="bg-primary-container text-on-primary-container">
        {sourceLabelOf(profile)}
      </ProfileCardChip>

      {remote && remote.option.update_interval_minutes > 0 && (
        <ProfileCardChip
          title={m.profile_card_auto_update_hint({
            minutes: remote.option.update_interval_minutes,
          })}
        >
          <RefreshRounded className="size-3.5" aria-hidden />
          {m.profile_card_auto_update({
            interval: updateIntervalLabel(
              remote.option.update_interval_minutes,
            ),
          })}
        </ProfileCardChip>
      )}

      {sourceCount > 0 && (
        <ProfileCardChip>
          {m.profile_card_source_count({ count: sourceCount })}
        </ProfileCardChip>
      )}

      {transforms.length > 0 && (
        <Popover>
          <PopoverTrigger asChild>
            <Button
              type="button"
              className="bg-surface-variant text-on-surface-variant inline-flex h-auto min-w-0 items-center gap-1 px-3 py-1 text-xs font-bold"
            >
              <LayersOutlineRounded className="size-3.5" aria-hidden />
              {m.profile_card_transform_count({ count: transforms.length })}
            </Button>
          </PopoverTrigger>
          <PopoverContent
            align="start"
            className="w-72 max-w-[calc(100vw-2rem)] p-4"
          >
            <p className="mb-3 text-sm font-medium">
              {m.profile_card_scoped_transforms_title()}
            </p>
            <ol
              className="max-h-64 space-y-2 overflow-y-auto"
              data-slot="profile-card-transform-details"
            >
              {transforms.map((uid, index) => {
                const transform = profiles.find((item) => item.uid === uid)

                return (
                  <li
                    key={`${uid}-${index}`}
                    className="flex items-start gap-2 text-sm"
                  >
                    <span className="text-on-surface-variant tabular-nums">
                      {index + 1}.
                    </span>
                    <div className="min-w-0">
                      <p className="break-words">{transform?.name ?? uid}</p>
                      {transform && (
                        <p className="text-on-surface-variant text-xs">
                          {transformLabelOf(transform)}
                        </p>
                      )}
                    </div>
                  </li>
                )
              })}
            </ol>
          </PopoverContent>
        </Popover>
      )}

      {transformLabel && <ProfileCardChip>{transformLabel}</ProfileCardChip>}
      {isGlobal && (
        <ProfileCardChip>{m.profile_global_label()}</ProfileCardChip>
      )}
    </div>
  )
}
