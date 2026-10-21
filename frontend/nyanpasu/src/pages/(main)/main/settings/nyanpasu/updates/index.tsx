import { type ReactNode } from 'react'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import UpdatePreferencesCard from './_modules/update-preferences-card'
import UpdateSourceSelector from './_modules/update-source-selector'
import UpdateStatusCard from './_modules/update-status-card'

export const Route = createFileRoute('/(main)/main/settings/nyanpasu/updates/')(
  {
    component: RouteComponent,
  },
)

// Cards keep their own height; the columns pack them without row gaps.
function UpdatesMasonryItem({ children }: { children: ReactNode }) {
  return (
    <div className="break-inside-avoid pb-4" data-slot="updates-masonry-item">
      {children}
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">
        {m.settings_label_updates()}
      </PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <div
          className="-mb-4 columns-1 gap-x-4 md:columns-2"
          data-slot="updates-masonry"
        >
          <UpdatesMasonryItem>
            <UpdateStatusCard />
          </UpdatesMasonryItem>

          <UpdatesMasonryItem>
            <UpdateSourceSelector />
          </UpdatesMasonryItem>

          <UpdatesMasonryItem>
            <UpdatePreferencesCard />
          </UpdatesMasonryItem>
        </div>
      </div>
    </>
  )
}
