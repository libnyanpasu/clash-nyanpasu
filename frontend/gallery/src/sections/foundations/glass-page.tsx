import FilterListRounded from '~icons/material-symbols/filter-list-rounded'
import PlayArrowRounded from '~icons/material-symbols/play-arrow-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import TuneRounded from '~icons/material-symbols/tune-rounded'
import { useState } from 'react'
import { glassSurfaceVariants } from '@nyanpasu/ui/glass-surface'
import {
  Button,
  GlassControlGroup,
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { PageHeader, SpecimenCard } from '../../studio/stage'

export function GlassPage() {
  const [filter, setFilter] = useState('all')

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-glass-page">
      <PageHeader
        eyebrow="Foundations · Android 17 Glass"
        title="Restrained Frosted Translucency"
        description="Dynamic-tinted frosted chrome (glassSurfaceVariants) is reserved strictly for floating overlays such as tool docks and pill controls. Reading panes and settings cards remain opaque tonal surfaces. Toggle Reduced Transparency in the bottom dock to verify solid fallbacks."
        badges={['glassSurfaceVariants', 'GlassControlGroup', 'A17 fallback']}
      />

      <SpecimenCard
        title="Floating Glass Control Dock Over Rich Content"
        description="Frosted glass inherits the dynamic surface-container-high tint at 76% alpha with backdrop-blur-xl, a subtle outline-variant/40 edge, and mandatory solid fallbacks."
        defaultSurface="glass"
        slots={['glass-control-group']}
        code={`<GlassControlGroup>
  <SegmentedButton type="single" variant="glass" value={filter} onValueChange={setFilter}>
    <SegmentedButtonItem value="all">All</SegmentedButtonItem>
    <SegmentedButtonItem value="active">Active</SegmentedButtonItem>
  </SegmentedButton>
  <Button variant="glass" icon aria-label="Refresh">
    <RefreshRounded className="size-4" />
  </Button>
</GlassControlGroup>`}
      >
        <div className="relative overflow-hidden rounded-2xl p-6">
          {/* Colorful content layer beneath floating chrome */}
          <div className="grid grid-cols-3 gap-3 opacity-80">
            <div className="bg-primary-container text-on-primary-container rounded-2xl p-4 text-xs font-medium">
              Primary Container Layer
            </div>
            <div className="bg-secondary-container text-on-secondary-container rounded-2xl p-4 text-xs font-medium">
              Secondary Container Layer
            </div>
            <div className="bg-tertiary-container text-on-tertiary-container rounded-2xl p-4 text-xs font-medium">
              Tertiary Container Layer
            </div>
            <div className="bg-surface-container-lowest dark:bg-surface-container col-span-3 rounded-2xl p-4 text-xs">
              Background rows remain calm, crisp, and opaque while floating
              chrome glides above them.
            </div>
          </div>

          <div className="mt-5 flex flex-wrap items-center justify-center gap-3">
            <GlassControlGroup>
              <SegmentedButton
                variant="glass"
                size="sm"
                value={filter}
                onValueChange={(next) => {
                  if (next) setFilter(next)
                }}
              >
                <SegmentedButtonItem value="all" className="px-3">
                  All
                </SegmentedButtonItem>
                <SegmentedButtonItem value="active" className="px-3">
                  Active
                </SegmentedButtonItem>
                <SegmentedButtonItem value="direct" className="px-3">
                  Direct
                </SegmentedButtonItem>
              </SegmentedButton>

              <Button variant="glass" icon aria-label="Filter">
                <FilterListRounded className="size-4" />
              </Button>
              <Button variant="glass" icon aria-label="Refresh">
                <RefreshRounded className="size-4" />
              </Button>
            </GlassControlGroup>

            <div
              data-glass-surface="true"
              className={cn(
                glassSurfaceVariants(),
                'text-on-surface flex h-11 items-center gap-2 px-4 text-xs font-medium',
              )}
            >
              <PlayArrowRounded className="text-primary size-4" />
              <span>Standalone Glass Pill</span>
              <TuneRounded className="text-on-surface-variant size-4" />
            </div>
          </div>
        </div>
      </SpecimenCard>
    </div>
  )
}
