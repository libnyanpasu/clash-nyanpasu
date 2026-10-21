import { type ReactNode } from 'react'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import NyanpasuVersion from './_modules/nyanpasu-version'
import OpenSourceCreditsCard from './_modules/open-source-credits-card'
import UpdatePreferencesCard from './_modules/update-preferences-card'
import UpdateSourceSelector from './_modules/update-source-selector'

export const Route = createFileRoute('/(main)/main/settings/about/')({
  component: RouteComponent,
})

// Cards keep their own height; the columns pack them without row gaps.
function AboutMasonryItem({ children }: { children: ReactNode }) {
  return (
    <div className="break-inside-avoid pb-4" data-slot="about-masonry-item">
      {children}
    </div>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle backTo="/main/settings">{m.settings_label_about()}</PageTitle>

      <div className="space-y-4 px-4 pb-4">
        <div
          className="-mb-4 columns-1 gap-x-4 md:columns-2"
          data-slot="about-masonry"
        >
          <AboutMasonryItem>
            <NyanpasuVersion />
          </AboutMasonryItem>

          <AboutMasonryItem>
            <UpdateSourceSelector />
          </AboutMasonryItem>

          <AboutMasonryItem>
            <UpdatePreferencesCard />
          </AboutMasonryItem>

          <AboutMasonryItem>
            <OpenSourceCreditsCard />
          </AboutMasonryItem>
        </div>
      </div>
    </>
  )
}
