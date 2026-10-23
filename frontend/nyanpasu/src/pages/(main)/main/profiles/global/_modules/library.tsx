import AddRounded from '~icons/material-symbols/add-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import PlaylistAddRounded from '~icons/material-symbols/playlist-add-rounded'
import SearchOffRounded from '~icons/material-symbols/search-off-rounded'
import { useId, useMemo, useState } from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { SearchField } from '@nyanpasu/ui/search-field'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { cn } from '@nyanpasu/utils'
import {
  CompactButton,
  RowAction,
  searchPillClassName,
} from '../../_modules/compact-controls'
import { ProfileType } from '../../_modules/consts'
import CreateProfileModal from '../../_modules/create-profile-modal'
import {
  StepGroup,
  StepRowIcon,
  StepRowText,
  stepRowVariants,
} from '../../_modules/step-list'
import TransformTypeIcon from '../../_modules/transform-type-icon'
import type { TransformChainDraft } from '../../_modules/use-transform-chain-draft'
import {
  profileTypeOf,
  transformLabelOf,
  type TransformProfile,
} from '../../_modules/utils'
import Column, { ColumnHead } from './column'

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
  usedBy,
  disabled,
  onAdd,
}: {
  profile: TransformProfile
  /** Names of the configs that run this transform. */
  usedBy: string[]
  disabled: boolean
  onAdd: (uid: string) => void
}) {
  return (
    <div data-slot="profile-global-library-row" className={stepRowVariants()}>
      <StepRowIcon>
        <TransformTypeIcon type={profileTypeOf(profile)} />
      </StepRowIcon>

      <StepRowText
        title={profile.name}
        sub={`${transformLabelOf(profile)} · ${
          usedBy.length > 0
            ? m.profile_global_library_used_by({ names: usedBy.join(', ') })
            : m.profile_global_library_unused()
        }`}
      />

      <RowAction
        label={m.profile_global_library_open_editor({ name: profile.name })}
        data-slot="profile-global-library-open-editor"
        onClick={async () => {
          await rpc.createEditorWindow('profile', profile.uid)
        }}
      >
        <EditSquareOutlineRounded className="size-5" aria-hidden />
      </RowAction>

      <RowAction
        label={m.profile_global_library_add({ name: profile.name })}
        disabled={disabled}
        data-slot="profile-global-library-add"
        onClick={() => onAdd(profile.uid)}
      >
        <AddRounded className="size-5" aria-hidden />
      </RowAction>
    </div>
  )
}

/** Every transform that is not in the global chain yet, ready to be added. */
export default function Library({
  chain,
  usedByOf,
}: {
  chain: TransformChainDraft
  /** Names of the configs that run the transform with this uid. */
  usedByOf: (uid: string) => string[]
}) {
  const headingId = useId()
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
    <Column
      data-slot="profile-global-library"
      aria-labelledby={headingId}
      className={cn(
        'bg-pane @[56rem]:flex-[0.85]',
        'shadow-[inset_0_1px_0_var(--color-outline-variant)]',
        '@[56rem]:shadow-[inset_1px_0_0_var(--color-outline-variant)]',
      )}
    >
      <ColumnHead id={headingId} title={m.profile_global_library_title()}>
        <CompactButton
          data-slot="profile-global-library-create"
          onClick={() => setCreateOpen(true)}
        >
          <AddRounded aria-hidden />
          {m.profile_create_transform_title()}
        </CompactButton>
      </ColumnHead>

      <SearchField
        className={searchPillClassName}
        value={search}
        onValueChange={setSearch}
        placeholder={m.profile_global_library_search()}
        aria-label={m.profile_global_library_search()}
        clearLabel={m.common_clear()}
      />

      <div
        role="group"
        data-slot="profile-global-library-filters"
        aria-label={m.profile_global_library_filter()}
        className="flex flex-wrap gap-1.5 px-4"
      >
        {TYPE_FILTERS.map((item) => (
          <button
            key={item.value}
            type="button"
            aria-pressed={type === item.value}
            onClick={() => setType(item.value)}
            className={cn(
              'h-[30px] cursor-pointer rounded-lg px-3 text-xs font-medium outline-none',
              'text-on-surface-variant shadow-[inset_0_0_0_1px_var(--color-outline-variant)]',
              'hover:bg-on-surface/8 focus-visible:outline-primary focus-visible:outline-2',
              'aria-pressed:bg-secondary-container aria-pressed:text-on-secondary-container aria-pressed:shadow-none',
            )}
          >
            {item.label()}
          </button>
        ))}
      </div>

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
          <CompactButton onClick={() => setCreateOpen(true)}>
            {m.profile_create_transform_title()}
          </CompactButton>
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
        <StepGroup data-slot="profile-global-library-list">
          {matches.map((profile) => (
            <LibraryRow
              key={profile.uid}
              profile={profile}
              usedBy={usedByOf(profile.uid)}
              disabled={chain.actionsDisabled}
              onAdd={chain.handleAdd}
            />
          ))}
        </StepGroup>
      )}

      <p
        data-slot="profile-global-library-note"
        className="text-on-surface-variant px-4 text-xs leading-4"
      >
        {m.profile_global_library_note()}
      </p>

      <CreateProfileModal
        open={createOpen}
        onOpenChange={setCreateOpen}
        kind={ProfileType.JavaScript}
        source="local"
      />
    </Column>
  )
}
