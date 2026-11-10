import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { TransformLibrary } from '../../_modules/transform-library'
import type { TransformChainDraft } from '../../_modules/use-transform-chain-draft'
import Column from './column'

/** The library as the global page's pane, next to the pipeline. */
export default function LibraryColumn({
  chain,
  usedByOf,
}: {
  chain: TransformChainDraft
  /** Names of the configs that run the transform with this uid. */
  usedByOf: (uid: string) => string[]
}) {
  return (
    <Column
      data-slot="profile-global-library"
      className={cn(
        'bg-pane @[56rem]:flex-[0.85]',
        'shadow-[inset_0_1px_0_var(--color-outline-variant)]',
        '@[56rem]:shadow-[inset_1px_0_0_var(--color-outline-variant)]',
      )}
    >
      <TransformLibrary chain={chain} usedByOf={usedByOf} />

      <p
        data-slot="profile-global-library-note"
        className="text-on-surface-variant px-4 text-xs leading-4"
      >
        {m.profile_global_library_note()}
      </p>
    </Column>
  )
}
