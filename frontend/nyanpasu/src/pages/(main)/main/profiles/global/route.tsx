import { useMemo } from 'react'
import { m } from '@/paraglide/messages'
import { useProfile } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import { TransformChainEditor } from '../$type/detail/_modules/chian-editor-card'

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
    <section className="flex min-w-0 flex-1 flex-col gap-4 p-4 pt-0">
      <header className="bg-mixed-background sticky top-0 z-50 py-4">
        <h1 className="text-lg font-bold">
          {m.profile_global_transform_title()}
        </h1>
        <p className="text-on-surface-variant mt-1 text-sm">
          {m.profile_global_transform_hint()}
        </p>
      </header>

      <TransformChainEditor
        taskKey="update-global-transforms"
        transforms={transforms}
        onApply={(next) => setGlobalTransforms.mutateAsync(next)}
      />
    </section>
  )
}
