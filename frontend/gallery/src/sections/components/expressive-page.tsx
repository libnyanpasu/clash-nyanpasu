import BoltRounded from '~icons/material-symbols/bolt-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import RouteRounded from '~icons/material-symbols/route-rounded'
import ShieldRounded from '~icons/material-symbols/shield-rounded'
import TuneRounded from '~icons/material-symbols/tune-rounded'
import { useState } from 'react'
import {
  ExpressiveChoice,
  MaterialShape,
  SegmentedButton,
  SegmentedButtonItem,
  ShapeToggle,
  Slider,
  Switch,
} from '@nyanpasu/ui'
import { PageHeader, SpecimenCard } from '../../studio/stage'

export function ExpressivePage() {
  const [choice, setChoice] = useState('rule')
  const [choiceLayout, setChoiceLayout] = useState<'focus' | 'flex'>('focus')
  const [containerHeight, setContainerHeight] = useState(160)

  const [proxyOn, setProxyOn] = useState(true)
  const [tunOn, setTunOn] = useState(false)
  const [toggleLoading, setToggleLoading] = useState(false)

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-expressive-page">
      <PageHeader
        eyebrow="Components · Expressive"
        title="M3 Expressive Signature Primitives"
        description="Clash Nyanpasu's signature tactile controls combining container-query scaling, elastic flex proportions, and scalloped MaterialShape morphing."
        badges={['ExpressiveChoice', 'ShapeToggle', 'MaterialShape']}
      />

      <SpecimenCard
        title="ExpressiveChoice (Focus & Flex Layouts)"
        description="Adapts smoothly via @container height queries — drag the height slider below 110px to see compact density kick in without internal scrollbars."
        slots={[
          'expressive-choice',
          'expressive-choice-current',
          'expressive-choice-options',
        ]}
        controls={
          <div className="flex w-full flex-wrap items-center justify-between gap-4">
            <SegmentedButton
              variant="tabs"
              size="sm"
              className="w-48"
              value={choiceLayout}
              onValueChange={(next) => {
                if (next) setChoiceLayout(next as 'focus' | 'flex')
              }}
            >
              <SegmentedButtonItem value="focus">Focus</SegmentedButtonItem>
              <SegmentedButtonItem value="flex">Flex</SegmentedButtonItem>
            </SegmentedButton>

            <div className="flex min-w-52 flex-1 flex-col gap-1">
              <div className="flex justify-between text-xs">
                <span className="text-on-surface-variant font-medium">
                  Widget Height (@container test)
                </span>
                <span className="font-mono tabular-nums">
                  {containerHeight}px
                </span>
              </div>
              <Slider
                aria-label="ExpressiveChoice container height"
                min={84}
                max={220}
                step={4}
                value={containerHeight}
                onValueChange={setContainerHeight}
              />
            </div>
          </div>
        }
        code={`<ExpressiveChoice
  label="Proxy mode"
  layout="${choiceLayout}"
  value={mode}
  onChange={setMode}
  options={options}
/>`}
      >
        <div
          className="flex flex-col"
          style={{ height: `${containerHeight}px` }}
        >
          <ExpressiveChoice
            label="Proxy mode"
            layout={choiceLayout}
            value={choice}
            onChange={setChoice}
            options={[
              {
                value: 'rule',
                label: 'Rule',
                description: 'Route traffic by profile ruleset',
                icon: <RouteRounded />,
              },
              {
                value: 'global',
                label: 'Global',
                description: 'Send all connections through active proxy',
                icon: <PublicRounded />,
              },
              {
                value: 'direct',
                label: 'Direct',
                description: 'Connect directly without proxying',
                icon: <BoltRounded />,
              },
            ]}
          />
        </div>
      </SpecimenCard>

      <SpecimenCard
        title="ShapeToggle (Default, Compact & Elastic)"
        description="Morphs its MaterialShape emblem between soft polygon and cookie geometry on toggle, with built-in loading spinner and elastic flex-grow."
        slots={['shape-toggle', 'shape-toggle-surface', 'shape-toggle-label']}
        controls={
          <label className="flex items-center gap-2 text-xs font-medium">
            <Switch
              checked={toggleLoading}
              onCheckedChange={setToggleLoading}
            />
            <span>Loading state</span>
          </label>
        }
        code={`<ShapeToggle
  active={active}
  label="System Proxy"
  icon={<BoltRounded />}
  onClick={() => setActive(!active)}
/>`}
      >
        <div className="grid grid-cols-1 gap-6 md:grid-cols-3">
          <div className="flex flex-col gap-2">
            <span className="text-on-surface-variant text-xs font-medium">
              Default Tile
            </span>
            <div className="flex h-36 gap-2">
              <ShapeToggle
                active={proxyOn}
                loading={toggleLoading}
                label="System Proxy"
                icon={<BoltRounded />}
                onClick={() => setProxyOn((v) => !v)}
              />
              <ShapeToggle
                active={tunOn}
                loading={toggleLoading}
                label="TUN Mode"
                icon={<ShieldRounded />}
                onClick={() => setTunOn((v) => !v)}
              />
            </div>
          </div>

          <div className="flex flex-col gap-2">
            <span className="text-on-surface-variant text-xs font-medium">
              Compact Row
            </span>
            <div className="flex h-36 flex-col gap-2">
              <ShapeToggle
                compact
                active={proxyOn}
                loading={toggleLoading}
                label="System Proxy"
                icon={<BoltRounded />}
                onClick={() => setProxyOn((v) => !v)}
              />
              <ShapeToggle
                compact
                active={tunOn}
                loading={toggleLoading}
                label="TUN Mode"
                icon={<ShieldRounded />}
                onClick={() => setTunOn((v) => !v)}
              />
            </div>
          </div>

          <div className="flex flex-col gap-2">
            <span className="text-on-surface-variant text-xs font-medium">
              Elastic Pair
            </span>
            <div className="flex h-36 gap-2">
              <ShapeToggle
                elastic
                active={proxyOn}
                loading={toggleLoading}
                label="Proxy"
                icon={<BoltRounded />}
                onClick={() => setProxyOn((v) => !v)}
              />
              <ShapeToggle
                elastic
                active={tunOn}
                loading={toggleLoading}
                label="TUN"
                icon={<TuneRounded />}
                onClick={() => setTunOn((v) => !v)}
              />
            </div>
          </div>
        </div>
      </SpecimenCard>

      <SpecimenCard
        title="MaterialShape Emblems"
        description="Decorative scalloped polygon badges across accent roles."
        slots={['material-shape']}
      >
        <div className="flex flex-wrap items-center gap-6">
          {[
            {
              label: 'Primary',
              depth: 8,
              cls: 'bg-primary text-on-primary',
            },
            {
              label: 'Primary Container',
              depth: 6,
              cls: 'bg-primary-container text-on-primary-container',
            },
            {
              label: 'Secondary Container',
              depth: 10,
              cls: 'bg-secondary-container text-on-secondary-container',
            },
            {
              label: 'Tertiary Container',
              depth: 4,
              cls: 'bg-tertiary-container text-on-tertiary-container',
            },
          ].map((item) => (
            <div key={item.label} className="flex flex-col items-center gap-2">
              <MaterialShape
                depth={item.depth}
                className={`${item.cls} size-16`}
              >
                <BoltRounded className="size-6" />
              </MaterialShape>
              <span className="text-on-surface-variant text-xs">
                {item.label}
              </span>
            </div>
          ))}
        </div>
      </SpecimenCard>
    </div>
  )
}
