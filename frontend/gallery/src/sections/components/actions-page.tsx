import AddRounded from '~icons/material-symbols/add-rounded'
import BoltRounded from '~icons/material-symbols/bolt-rounded'
import CloudUploadRounded from '~icons/material-symbols/cloud-upload-rounded'
import DownloadRounded from '~icons/material-symbols/download-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import SaveRounded from '~icons/material-symbols/save-rounded'
import ShareRounded from '~icons/material-symbols/share-rounded'
import { useState } from 'react'
import {
  ActionSwapText,
  Button,
  FabMenu,
  FabMenuContent,
  FabMenuItem,
  FabMenuTrigger,
  GlassControlGroup,
  Ripple,
  SegmentedButton,
  SegmentedButtonItem,
  Switch,
  useRipple,
  type ButtonProps,
} from '@nyanpasu/ui'
import { PageHeader, SpecimenCard } from '../../studio/stage'

const BUTTON_VARIANTS: readonly NonNullable<ButtonProps['variant']>[] = [
  'flat',
  'fab',
  'raised',
  'stroked',
  'basic',
  'glass',
]

export function ActionsPage() {
  const [loading, setLoading] = useState(false)
  const [disabled, setDisabled] = useState(false)
  const [segmentedValue, setSegmentedValue] = useState('rule')
  const [tabsValue, setTabsValue] = useState('daily')
  const [swapState, setSwapState] = useState<'Apply Changes' | 'Saved!'>(
    'Apply Changes',
  )
  const ripple = useRipple()
  const handleRippleClick = ripple.onClick
  const handleRippleClear = ripple.onClear

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-actions-page">
      <PageHeader
        eyebrow="Components · Actions"
        title="Actions, Buttons & Floating Controls"
        description="Hierarchy-driven controls from high-emphasis flat/FAB actions to restrained glass dock controls, segmented button groups, action label swaps, and tactile ripples."
        badges={[
          'Button',
          'FabMenu',
          'SegmentedButton',
          'GlassControlGroup',
          'ActionSwapText',
          'Ripple',
        ]}
      />

      <SpecimenCard
        title="Button Variants & State Matrix"
        description="Buttons default to single-line inline-flex layout (items-center justify-center gap-2 whitespace-nowrap) so labels and icons stay aligned without wrapping. Reserve flat and fab for one primary CTA per screen, and opt into whitespace-normal only for explicit multi-line buttons."
        slots={['button-content', 'button-loading']}
        controls={
          <div className="flex flex-wrap items-center gap-6 text-xs font-medium">
            <label className="flex items-center gap-2">
              <Switch checked={loading} onCheckedChange={setLoading} />
              <span>Loading</span>
            </label>
            <label className="flex items-center gap-2">
              <Switch checked={disabled} onCheckedChange={setDisabled} />
              <span>Disabled</span>
            </label>
          </div>
        }
        code={`<Button variant="flat" loading={loading} disabled={disabled}>
  <SaveRounded className="size-4" />
  <span>Save Profile</span>
</Button>
<Button variant="fab" icon aria-label="Add">
  <AddRounded className="size-6" />
</Button>`}
      >
        <div className="flex flex-col gap-4">
          {BUTTON_VARIANTS.map((variant) => (
            <div
              key={variant}
              className="flex flex-wrap items-center justify-between gap-3"
            >
              <span className="text-on-surface-variant w-20 font-mono text-xs">
                {variant}
              </span>

              <div className="flex flex-1 flex-wrap items-center gap-3">
                <Button variant={variant} loading={loading} disabled={disabled}>
                  Label
                </Button>

                <Button variant={variant} loading={loading} disabled={disabled}>
                  <SaveRounded className="size-4" />
                  <span>With Icon</span>
                </Button>

                <Button
                  variant={variant}
                  icon
                  loading={loading}
                  disabled={disabled}
                  aria-label={`${variant} icon action`}
                >
                  <AddRounded className="size-5" />
                </Button>
              </div>
            </div>
          ))}
        </div>
      </SpecimenCard>

      <SpecimenCard
        title="SegmentedButton & Connected Groups"
        description="Supports filled (bordered connected pills), tabs (sliding indicator), and glass (floating dock) variants."
        slots={['segmented-button-slider-pill']}
        code={`<SegmentedButton type="single" variant="filled" value={value} onValueChange={setValue}>
  <SegmentedButtonItem value="rule">Rule</SegmentedButtonItem>
  <SegmentedButtonItem value="global">Global</SegmentedButtonItem>
  <SegmentedButtonItem value="direct">Direct</SegmentedButtonItem>
</SegmentedButton>`}
      >
        <div className="flex flex-col gap-5">
          <div className="max-w-md">
            <div className="text-on-surface-variant mb-2 text-xs font-medium">
              Filled Connected Group
            </div>
            <SegmentedButton
              variant="filled"
              value={segmentedValue}
              onValueChange={(next) => {
                if (next) setSegmentedValue(next)
              }}
            >
              <SegmentedButtonItem value="rule">Rule</SegmentedButtonItem>
              <SegmentedButtonItem value="global">Global</SegmentedButtonItem>
              <SegmentedButtonItem value="direct">Direct</SegmentedButtonItem>
            </SegmentedButton>
          </div>

          <div className="max-w-md">
            <div className="text-on-surface-variant mb-2 text-xs font-medium">
              Tabs Sliding Indicator
            </div>
            <SegmentedButton
              variant="tabs"
              size="sm"
              value={tabsValue}
              onValueChange={(next) => {
                if (next) setTabsValue(next)
              }}
            >
              <SegmentedButtonItem value="hourly">1h</SegmentedButtonItem>
              <SegmentedButtonItem value="daily">24h</SegmentedButtonItem>
              <SegmentedButtonItem value="weekly">7d</SegmentedButtonItem>
            </SegmentedButton>
          </div>
        </div>
      </SpecimenCard>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <SpecimenCard
          title="FabMenu"
          description="Material 3 FAB that expands into a staggered stack of pill actions and morphs its trigger into a round close button."
          code={`<FabMenu>
  <FabMenuTrigger icon={<AddRounded className="size-6" />} aria-label="Actions" />
  <FabMenuContent>
    <FabMenuItem icon={<CloudUploadRounded />}>Import URL</FabMenuItem>
    <FabMenuItem icon={<EditRounded />}>New Local Profile</FabMenuItem>
  </FabMenuContent>
</FabMenu>`}
        >
          <div className="flex min-h-36 items-end justify-end p-2">
            <FabMenu>
              <FabMenuTrigger
                icon={<AddRounded className="size-6" />}
                aria-label="Open FAB menu"
              />
              <FabMenuContent>
                <FabMenuItem icon={<CloudUploadRounded className="size-5" />}>
                  Import Remote URL
                </FabMenuItem>
                <FabMenuItem icon={<EditRounded className="size-5" />}>
                  Create Local Profile
                </FabMenuItem>
                <FabMenuItem icon={<DownloadRounded className="size-5" />}>
                  Restore Backup
                </FabMenuItem>
              </FabMenuContent>
            </FabMenu>
          </div>
        </SpecimenCard>

        <SpecimenCard
          title="ActionSwapText, GlassControlGroup & Ripple"
          description="Animated button copy transitions, grouped floating controls, and standalone ink ripples."
          slots={['glass-control-group']}
          defaultSurface="glass"
        >
          <div className="flex flex-col items-start gap-5">
            <Button
              variant="flat"
              onClick={() =>
                setSwapState((current) =>
                  current === 'Apply Changes' ? 'Saved!' : 'Apply Changes',
                )
              }
            >
              <ActionSwapText value={swapState} />
            </Button>

            <GlassControlGroup>
              <Button variant="glass" className="gap-1.5">
                <BoltRounded className="size-4" />
                <span>Test Latency</span>
              </Button>
              <Button variant="glass" icon aria-label="Refresh">
                <RefreshRounded className="size-4" />
              </Button>
              <Button variant="glass" icon aria-label="Share">
                <ShareRounded className="size-4" />
              </Button>
            </GlassControlGroup>

            <button
              type="button"
              onClick={handleRippleClick}
              className="bg-primary-container text-on-primary-container rounded-large relative overflow-hidden px-5 py-3 text-sm font-medium"
            >
              <span>Click anywhere for standalone Ripple</span>
              <Ripple ripples={ripple.ripples} onClear={handleRippleClear} />
            </button>
          </div>
        </SpecimenCard>
      </div>
    </div>
  )
}
