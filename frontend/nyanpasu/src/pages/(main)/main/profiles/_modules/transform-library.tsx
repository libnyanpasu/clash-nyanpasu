import AddRounded from '~icons/material-symbols/add-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import PlaylistAddRounded from '~icons/material-symbols/playlist-add-rounded'
import SearchOffRounded from '~icons/material-symbols/search-off-rounded'
import { cva } from 'class-variance-authority'
import { useId, useMemo, useState } from 'react'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import { Modal, ModalTrigger } from '@nyanpasu/ui/modal'
import { SearchField } from '@nyanpasu/ui/search-field'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { cn } from '@nyanpasu/utils'
import {
  CompactButton,
  RowAction,
  searchPillClassName,
} from './compact-controls'
import { ProfileType } from './consts'
import CreateProfileModal from './create-profile-modal'
import { StepGroup, StepRowText, stepRowVariants } from './step-list'
import { TransformTypeBadge } from './transform-type-icon'
import type { TransformChainDraft } from './use-transform-chain-draft'
import { profileTypeOf, transformLabelOf, type TransformProfile } from './utils'

const ALL = 'all'

type TypeFilter = typeof ALL | ProfileType

const TYPE_FILTERS = [
  { value: ALL, label: () => m.connections_tab_all() },
  { value: ProfileType.JavaScript, label: () => 'JavaScript' },
  { value: ProfileType.Lua, label: () => 'Lua' },
  { value: ProfileType.Merge, label: () => 'YAML' },
] as const satisfies readonly { value: TypeFilter; label: () => string }[]

const libraryToggleVariants = cva([
  'group/toggle',
  // A transform in the chain reads as a filled tonal check.
  'aria-pressed:bg-secondary-container aria-pressed:text-on-secondary-container',
  'dark:aria-pressed:bg-secondary-container dark:aria-pressed:text-on-secondary-container',
  'aria-pressed:hover:bg-secondary-container aria-pressed:hover:brightness-95',
  'dark:aria-pressed:hover:bg-secondary-container dark:aria-pressed:hover:brightness-110',
])

// The plus turns into the check, and back, as the transform joins the chain.
const libraryToggleIconVariants = cva(
  'size-5 transition-[opacity,rotate,scale] duration-200 motion-reduce:transition-none',
  {
    variants: {
      shows: {
        add: [
          'group-aria-pressed/toggle:scale-50 group-aria-pressed/toggle:rotate-90',
          'group-aria-pressed/toggle:opacity-0',
        ],
        check: [
          'absolute inset-0 m-auto scale-50 -rotate-90 opacity-0',
          'group-aria-pressed/toggle:scale-100 group-aria-pressed/toggle:rotate-0',
          'group-aria-pressed/toggle:opacity-100',
        ],
      },
    },
  },
)

function LibraryRow({
  profile,
  position,
  usedBy,
  disabled,
  onAdd,
  onRemove,
}: {
  profile: TransformProfile
  /** The 1-based place in the chain, or nothing when it is not in it. */
  position?: number
  /** Names of the configs that run this transform, when the caller knows. */
  usedBy?: string[]
  disabled: boolean
  onAdd: (uid: string) => void
  onRemove: (uid: string) => void
}) {
  const inChain = position !== undefined

  const supporting = [
    transformLabelOf(profile),
    inChain && m.profile_transform_library_position({ position }),
    usedBy &&
      (usedBy.length > 0
        ? m.profile_global_library_used_by({ names: usedBy.join(', ') })
        : m.profile_global_library_unused()),
  ]
    .filter(Boolean)
    .join(' · ')

  return (
    <div
      data-slot="profile-transform-library-row"
      data-uid={profile.uid}
      className={stepRowVariants()}
    >
      <TransformTypeBadge type={profileTypeOf(profile)} />

      <StepRowText title={profile.name} sub={supporting} />

      <RowAction
        label={m.profile_global_library_open_editor({ name: profile.name })}
        data-slot="profile-transform-library-open-editor"
        onClick={async () => {
          await rpc.createEditorWindow('profile', profile.uid)
        }}
      >
        <EditSquareOutlineRounded className="size-5" aria-hidden />
      </RowAction>

      <RowAction
        label={m.profile_transform_library_toggle({ name: profile.name })}
        aria-pressed={inChain}
        disabled={disabled}
        data-slot="profile-transform-library-toggle"
        className={libraryToggleVariants()}
        onClick={() => (inChain ? onRemove(profile.uid) : onAdd(profile.uid))}
      >
        <AddRounded
          className={libraryToggleIconVariants({ shows: 'add' })}
          aria-hidden
        />

        <CheckRounded
          className={libraryToggleIconVariants({ shows: 'check' })}
          aria-hidden
        />
      </RowAction>
    </div>
  )
}

/**
 * Every transform, with the ones in the chain marked: the list stays put while
 * transforms are added and removed. The caller frames it.
 */
export function TransformLibrary({
  chain,
  usedByOf,
  className,
}: {
  chain: TransformChainDraft
  /** Names of the configs that run the transform with this uid. */
  usedByOf?: (uid: string) => string[]
  className?: string
}) {
  const headingId = useId()
  const [search, setSearch] = useState('')
  const [type, setType] = useState<TypeFilter>(ALL)

  const positions = useMemo(
    () =>
      new Map(chain.enabled.map((profile, index) => [profile.uid, index + 1])),
    [chain.enabled],
  )

  // The filters count what the search leaves of each type.
  const searched = useMemo(() => {
    const term = search.trim().toLowerCase()

    return chain.candidates.filter((profile) =>
      profile.name.toLowerCase().includes(term),
    )
  }, [chain.candidates, search])

  const matches = useMemo(
    () =>
      searched.filter(
        (profile) => type === ALL || profileTypeOf(profile) === type,
      ),
    [searched, type],
  )

  const countOf = (filter: TypeFilter) =>
    filter === ALL
      ? searched.length
      : searched.filter((profile) => profileTypeOf(profile) === filter).length

  return (
    <Modal>
      <section
        data-slot="profile-transform-library"
        aria-labelledby={headingId}
        className={cn('flex min-w-0 flex-col gap-4', className)}
      >
        <div className="flex min-h-8 items-center gap-2 pl-4">
          <h2
            id={headingId}
            className="text-on-surface min-w-0 flex-1 truncate text-sm font-semibold"
          >
            {m.profile_global_library_title()}
          </h2>

          <ModalTrigger asChild>
            <CompactButton data-slot="profile-transform-library-create">
              <AddRounded aria-hidden />
              {m.profile_create_transform_title()}
            </CompactButton>
          </ModalTrigger>
        </div>

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
          data-slot="profile-transform-library-filters"
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
                'flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-xs font-medium outline-none',
                'text-on-surface-variant shadow-[inset_0_0_0_1px_var(--color-outline-variant)]',
                'hover:bg-on-surface/8 focus-visible:outline-primary focus-visible:outline-2',
                'transition-[background-color,color,box-shadow] motion-reduce:transition-none',
                'aria-pressed:bg-secondary-container aria-pressed:text-on-secondary-container aria-pressed:shadow-none',
              )}
            >
              {item.label()}

              <span className="tabular-nums opacity-70">
                {countOf(item.value)}
              </span>
            </button>
          ))}
        </div>

        {chain.candidates.length === 0 ? (
          <EmptyState
            size="compact"
            tone="normal"
            data-slot="profile-transform-library-empty"
            className="min-h-40 py-5"
            icon={PlaylistAddRounded}
            message={m.profile_global_library_empty()}
            description={m.profile_global_library_empty_hint()}
          >
            <ModalTrigger asChild>
              <CompactButton>
                {m.profile_create_transform_title()}
              </CompactButton>
            </ModalTrigger>
          </EmptyState>
        ) : matches.length === 0 ? (
          <EmptyState
            size="compact"
            tone="normal"
            data-slot="profile-transform-library-no-match"
            className="min-h-40 py-5"
            icon={SearchOffRounded}
            message={m.profile_global_library_no_match()}
          />
        ) : (
          <StepGroup data-slot="profile-transform-library-list">
            {matches.map((profile) => (
              <LibraryRow
                key={profile.uid}
                profile={profile}
                position={positions.get(profile.uid)}
                usedBy={usedByOf?.(profile.uid)}
                disabled={chain.actionsDisabled}
                onAdd={chain.handleAdd}
                onRemove={chain.handleRemove}
              />
            ))}
          </StepGroup>
        )}

        <CreateProfileModal kind={ProfileType.JavaScript} source="local" />
      </section>
    </Modal>
  )
}
