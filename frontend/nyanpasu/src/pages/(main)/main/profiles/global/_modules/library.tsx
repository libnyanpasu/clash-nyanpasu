import AddRounded from '~icons/material-symbols/add-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import PlaylistAddRounded from '~icons/material-symbols/playlist-add-rounded'
import SearchOffRounded from '~icons/material-symbols/search-off-rounded'
import { useMemo, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { SearchField } from '@nyanpasu/ui/search-field'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { ProfileType } from '../../_modules/consts'
import CreateProfileModal from '../../_modules/create-profile-modal'
import type { TransformChainDraft } from '../../_modules/use-transform-chain-draft'
import {
  profileTypeOf,
  transformLabelOf,
  type TransformProfile,
} from '../../_modules/utils'
import Panel from './panel'
import { RowTitle, rowVariants } from './row'

const ALL = 'all'

type TypeFilter = typeof ALL | ProfileType

const TYPE_FILTERS = [
  { value: ALL, label: () => m.connections_tab_all() },
  { value: ProfileType.JavaScript, label: () => 'JavaScript' },
  { value: ProfileType.Lua, label: () => 'Lua' },
  { value: ProfileType.Merge, label: () => 'YAML' },
] as const satisfies readonly { value: TypeFilter; label: () => string }[]

function LibraryRow({
  profile,
  disabled,
  onAdd,
}: {
  profile: TransformProfile
  disabled: boolean
  onAdd: (uid: string) => void
}) {
  return (
    <li data-slot="profile-global-library-row" className={rowVariants()}>
      <RowTitle name={profile.name} type={transformLabelOf(profile)} />

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            icon
            className="text-on-surface-variant size-9 shrink-0"
            data-slot="profile-global-library-open-editor"
            aria-label={m.profile_global_library_open_editor({
              name: profile.name,
            })}
            onClick={async () => {
              await rpc.createEditorWindow('profile', profile.uid)
            }}
          >
            <EditSquareOutlineRounded className="size-5" aria-hidden />
          </Button>
        </TooltipTrigger>

        <TooltipContent>{m.profile_chain_editor_open_editor()}</TooltipContent>
      </Tooltip>

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            icon
            disabled={disabled}
            className="text-primary size-9 shrink-0"
            data-slot="profile-global-library-add"
            aria-label={m.profile_global_library_add({ name: profile.name })}
            onClick={() => onAdd(profile.uid)}
          >
            <AddRounded className="size-5" aria-hidden />
          </Button>
        </TooltipTrigger>

        <TooltipContent>{m.profile_chain_editor_add()}</TooltipContent>
      </Tooltip>
    </li>
  )
}

/** Every transform that is not in the global chain yet, ready to be added. */
export default function Library({ chain }: { chain: TransformChainDraft }) {
  const [search, setSearch] = useState('')
  const [type, setType] = useState<TypeFilter>(ALL)
  const [createOpen, setCreateOpen] = useState(false)

  const matches = useMemo(() => {
    const term = search.trim().toLowerCase()

    return chain.available.filter(
      (profile) =>
        (type === ALL || profileTypeOf(profile) === type) &&
        profile.name.toLowerCase().includes(term),
    )
  }, [chain.available, search, type])

  const nothingExists = chain.available.length + chain.enabled.length === 0

  return (
    <Panel
      data-slot="profile-global-library"
      title={m.profile_global_library_title()}
      action={
        <Button
          type="button"
          data-slot="profile-global-library-create"
          onClick={() => setCreateOpen(true)}
        >
          <AddRounded className="size-5" aria-hidden />
          {m.profile_create_transform_title()}
        </Button>
      }
      toolbar={
        <div
          data-slot="profile-global-library-filters"
          className="flex shrink-0 flex-wrap items-center gap-2 px-1"
        >
          <SearchField
            className="min-w-[min(12rem,100%)] flex-1 basis-48"
            value={search}
            onValueChange={setSearch}
            placeholder={m.profile_global_library_search()}
            aria-label={m.profile_global_library_search()}
            clearLabel={m.common_clear()}
          />

          <SegmentedButton
            size="sm"
            variant="glass"
            value={type}
            aria-label={m.profile_global_library_filter()}
            onValueChange={(next) => {
              // Selecting the selected segment again clears a toggle group.
              const filter = TYPE_FILTERS.find((item) => item.value === next)
              if (filter) setType(filter.value)
            }}
          >
            {TYPE_FILTERS.map((item) => (
              <SegmentedButtonItem key={item.value} value={item.value}>
                {item.label()}
              </SegmentedButtonItem>
            ))}
          </SegmentedButton>
        </div>
      }
    >
      {nothingExists ? (
        <EmptyState
          size="compact"
          tone="normal"
          data-slot="profile-global-library-empty"
          className="min-h-40 py-5"
          icon={PlaylistAddRounded}
          message={m.profile_global_library_empty()}
          description={m.profile_global_library_empty_hint()}
        >
          <Button type="button" onClick={() => setCreateOpen(true)}>
            {m.profile_create_transform_title()}
          </Button>
        </EmptyState>
      ) : chain.available.length === 0 ? (
        <EmptyState
          size="compact"
          tone="normal"
          data-slot="profile-global-library-all-enabled"
          className="min-h-40 py-5"
          icon={PlaylistAddRounded}
          message={m.profile_chain_editor_add_empty()}
        />
      ) : matches.length === 0 ? (
        <EmptyState
          size="compact"
          tone="normal"
          data-slot="profile-global-library-no-match"
          className="min-h-40 py-5"
          icon={SearchOffRounded}
          message={m.profile_global_library_no_match()}
        />
      ) : (
        <ul
          data-slot="profile-global-library-list"
          className="flex min-w-0 flex-col gap-2 px-1 pb-2"
        >
          {matches.map((profile) => (
            <LibraryRow
              key={profile.uid}
              profile={profile}
              disabled={chain.actionsDisabled}
              onAdd={chain.handleAdd}
            />
          ))}
        </ul>
      )}

      <CreateProfileModal
        open={createOpen}
        onOpenChange={setCreateOpen}
        kind={ProfileType.JavaScript}
        source="local"
      />
    </Panel>
  )
}
