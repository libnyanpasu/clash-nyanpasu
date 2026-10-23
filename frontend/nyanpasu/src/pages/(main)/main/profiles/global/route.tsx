import { useMemo } from 'react'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { useProfile } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import { TransformChainList } from '../_modules/transform-chain-list'

export const Route = createFileRoute('/(main)/main/profiles/global')({
  component: RouteComponent,
})

function RouteComponent() {
  const { query, setGlobalTransforms } = useProfile()

  const transforms = useMemo(
    () => query.data?.global_transforms ?? [],
    [query.data?.global_transforms],
  )

  return (
    <section
      data-slot="profile-global-transforms"
      className="flex min-w-0 flex-1 flex-col"
    >
      <PageTitle backTo="/main/profiles">
        {m.profile_global_transform_title()}
      </PageTitle>

      <div className="flex flex-col gap-4 px-4 pb-4">
        <p className="text-on-surface-variant text-sm">
          {m.profile_global_transform_hint()}
        </p>

        <TransformChainList
          taskKey="update-global-transforms"
          traceStepOf={(uid, index) => `global:${uid}:${index}`}
          transforms={transforms}
          onApply={(next) => setGlobalTransforms.mutateAsync(next)}
        />
      </div>
    </section>
  )
}
