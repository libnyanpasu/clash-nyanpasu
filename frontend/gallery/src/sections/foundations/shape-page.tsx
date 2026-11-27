import { useState } from 'react'
import { SHAPE_SCALE } from '@nyanpasu/theme/spec'
import {
  Card,
  CardContent,
  CardHeader,
  MaterialShape,
  Slider,
  Switch,
} from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import { PageHeader, SpecimenCard } from '../../studio/stage'

export function ShapePage() {
  const [outerRadius, setOuterRadius] = useState(28)
  const [padding, setPadding] = useState(12)
  const [cookieDepth, setCookieDepth] = useState(8)
  const [shapeActive, setShapeActive] = useState(true)

  const innerRadius = Math.max(4, outerRadius - padding)

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-shape-page">
      <PageHeader
        eyebrow="Foundations · Shape"
        title="10-Step Shape Scale & Optical Roundness"
        description="M3 Expressive adds bold 20px, 32px, and 48px radii paired with Clash Nyanpasu's superellipse(1.25) corner geometry, asymmetric grouped list corners, and MaterialShape polygon morphing."
        badges={['SHAPE_SCALE', 'superellipse(1.25)', 'outer - padding']}
      />

      {/* 10-step scale */}
      <Card>
        <CardHeader className="flex flex-col items-start gap-1 px-6 pt-6 pb-0">
          <h2 className="text-on-surface text-base font-semibold">
            10-Step M3E Corner Scale
          </h2>
          <p className="text-on-surface-variant text-xs">
            Highlighted badges indicate the three May 2025 Expressive additions
            (20px, 32px, 48px).
          </p>
        </CardHeader>

        <CardContent className="grid grid-cols-2 gap-3 px-6 pt-4 pb-6 sm:grid-cols-3 lg:grid-cols-5">
          {SHAPE_SCALE.map((token) => (
            <div
              key={token.id}
              className="bg-surface-container dark:bg-surface-container-high flex flex-col justify-between gap-3 rounded-2xl p-4"
            >
              <div className="flex items-center justify-between gap-2">
                <span className="font-mono text-xs font-semibold">
                  {token.px === 9999 ? 'full' : `${token.px}px`}
                </span>
                {token.expressiveAddition && (
                  <span className="bg-tertiary-container text-on-tertiary-container rounded-full px-2 py-0.5 text-[10px] font-semibold">
                    M3E
                  </span>
                )}
              </div>

              <div
                className={cn(
                  'bg-primary-container text-on-primary-container grid h-16 place-items-center text-xs font-medium',
                  token.tailwindClass,
                )}
              >
                {token.id}
              </div>

              <div>
                <code className="text-on-surface-variant text-[11px]">
                  {token.tailwindClass}
                </code>
                <p className="text-on-surface-variant mt-1 text-[11px] leading-4">
                  {token.usage}
                </p>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      {/* Interactive Nested Radius Calculator */}
      <SpecimenCard
        title="Optical Roundness Calculator (inner = max(4, outer - padding))"
        description="Never copy the parent container's radius onto a padded child. Adjust outer radius and padding to inspect optical concentricity."
        allowSurfaceSwitch={false}
        controls={
          <div className="grid w-full grid-cols-1 gap-4 sm:grid-cols-2">
            <div className="flex flex-col gap-1">
              <div className="flex justify-between text-xs">
                <span className="text-on-surface-variant font-medium">
                  Outer Radius
                </span>
                <span className="font-mono tabular-nums">{outerRadius}px</span>
              </div>
              <Slider
                aria-label="Outer radius"
                min={8}
                max={48}
                step={2}
                value={outerRadius}
                onValueChange={setOuterRadius}
              />
            </div>

            <div className="flex flex-col gap-1">
              <div className="flex justify-between text-xs">
                <span className="text-on-surface-variant font-medium">
                  Container Padding
                </span>
                <span className="font-mono tabular-nums">{padding}px</span>
              </div>
              <Slider
                aria-label="Padding"
                min={4}
                max={28}
                step={2}
                value={padding}
                onValueChange={setPadding}
              />
            </div>
          </div>
        }
      >
        <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
          <div className="flex flex-col gap-2">
            <span className="text-ok text-xs font-semibold">
              Correct: inner = {innerRadius}px
            </span>
            <div
              className="bg-surface-container-lowest dark:bg-surface-container border-outline-variant/40 border"
              style={{
                borderRadius: `${outerRadius}px`,
                padding: `${padding}px`,
              }}
            >
              <div
                className="bg-primary-container text-on-primary-container grid h-28 place-items-center text-sm font-medium"
                style={{ borderRadius: `${innerRadius}px` }}
              >
                Concentric ({outerRadius} - {padding} = {innerRadius}px)
              </div>
            </div>
          </div>

          <div className="flex flex-col gap-2">
            <span className="text-error text-xs font-semibold">
              Anti-pattern: copied outer = {outerRadius}px
            </span>
            <div
              className="bg-surface-container-lowest dark:bg-surface-container border-outline-variant/40 border"
              style={{
                borderRadius: `${outerRadius}px`,
                padding: `${padding}px`,
              }}
            >
              <div
                className="bg-error-container text-on-error-container grid h-28 place-items-center text-sm font-medium"
                style={{ borderRadius: `${outerRadius}px` }}
              >
                Pinched corners ({outerRadius}px inside {outerRadius}px)
              </div>
            </div>
          </div>
        </div>
      </SpecimenCard>

      {/* MaterialShape Interactive Morph Lab */}
      <SpecimenCard
        title="MaterialShape Scalloped Cookie Morph"
        description="Used sparingly for active navigation emblems, ExpressiveChoice heroes, and ShapeToggle focal points — never on text-heavy cards."
        slots={['material-shape']}
        controls={
          <div className="flex w-full flex-wrap items-center justify-between gap-4">
            <div className="flex min-w-52 flex-1 flex-col gap-1">
              <div className="flex justify-between text-xs">
                <span className="text-on-surface-variant font-medium">
                  Custom Depth
                </span>
                <span className="font-mono tabular-nums">{cookieDepth}</span>
              </div>
              <Slider
                aria-label="Cookie depth"
                min={0}
                max={12}
                step={1}
                value={cookieDepth}
                onValueChange={setCookieDepth}
              />
            </div>

            <label className="flex items-center gap-2 text-xs font-medium">
              <Switch checked={shapeActive} onCheckedChange={setShapeActive} />
              <span>Active state</span>
            </label>
          </div>
        }
        code={`<MaterialShape active={active} depth={8} className="bg-primary text-on-primary size-20" />`}
      >
        <div className="flex flex-wrap items-center justify-around gap-6 py-4">
          <div className="flex flex-col items-center gap-2">
            <MaterialShape
              depth={cookieDepth}
              className="bg-primary text-on-primary size-24 text-sm font-semibold"
            >
              d={cookieDepth}
            </MaterialShape>
            <span className="text-on-surface-variant text-xs">
              Parameterized depth
            </span>
          </div>

          <div className="flex flex-col items-center gap-2">
            <MaterialShape
              active={shapeActive}
              className="bg-secondary-container text-on-secondary-container size-24 text-sm font-semibold"
            >
              {shapeActive ? 'Cookie' : 'Soft'}
            </MaterialShape>
            <span className="text-on-surface-variant text-xs">
              Default active toggle
            </span>
          </div>

          <div className="flex flex-col items-center gap-2">
            <MaterialShape
              active={shapeActive}
              inactiveSquare
              className="bg-tertiary-container text-on-tertiary-container size-24 text-sm font-semibold"
            >
              {shapeActive ? 'Cookie' : 'Square'}
            </MaterialShape>
            <span className="text-on-surface-variant text-xs">
              inactiveSquare mode
            </span>
          </div>
        </div>
      </SpecimenCard>
    </div>
  )
}
