import BoltRounded from '~icons/material-symbols/bolt-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import { useState } from 'react'
import {
  AnimatedItem,
  Button,
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  Circle,
  CircleSVG,
  EmptyState,
  HighlightText,
  Kbd,
  LazyImage,
  SearchField,
  Sparkline,
  TextMarquee,
} from '@nyanpasu/ui'
import { PageHeader, SpecimenCard } from '../../studio/stage'

const INITIAL_SERIES = [18, 24, 21, 35, 42, 39, 54, 48, 63, 58, 72, 65]

const INLINE_SVG_PREVIEW =
  'data:image/svg+xml,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 120"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#3e64a7"/><stop offset="100%" stop-color="#685781"/></linearGradient></defs><rect width="320" height="120" rx="20" fill="url(#g)"/><circle cx="250" cy="60" r="36" fill="rgba(255,255,255,0.22)"/><text x="24" y="68" fill="#ffffff" font-family="sans-serif" font-size="18" font-weight="600">LazyImage SVG</text></svg>',
  )

export function SurfacesDataPage() {
  const [series, setSeries] = useState(INITIAL_SERIES)
  const [highlightQuery, setHighlightQuery] = useState('tokyo')
  const [animatedKey, setAnimatedKey] = useState(0)

  const pushSample = () => {
    setSeries((current) => {
      const next = Math.max(
        10,
        Math.min(
          95,
          current[current.length - 1] + Math.round((Math.random() - 0.45) * 28),
        ),
      )
      return [...current.slice(1), next]
    })
  }

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-surfaces-data-page">
      <PageHeader
        eyebrow="Components · Surfaces & Data"
        title="Cards, Sparklines & Data Display"
        description="Tonal cards without shadow clutter, D3 Catmull-Rom sparklines, search highlighting, auto-marquee overflow text, keyboard badges, circular meters, and empty states."
        badges={[
          'Card',
          'Sparkline',
          'HighlightText',
          'TextMarquee',
          'Kbd',
          'Circle',
          'LazyImage',
          'EmptyState',
          'AnimatedItem',
        ]}
      />

      <SpecimenCard
        title="Card Variants (Basic, Raised, Outline)"
        description="Basic cards flip tone polarity cleanly between light (surface-container-lowest) and dark (surface-container)."
        defaultSurface="canvas"
        code={`<Card variant="basic" divider>
  <CardHeader>Profile Summary</CardHeader>
  <CardContent>...</CardContent>
  <CardFooter>
    <Button variant="flat">Apply</Button>
  </CardFooter>
</Card>`}
      >
        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          {(['basic', 'raised', 'outline'] as const).map((variant) => (
            <Card key={variant} variant={variant} divider>
              <CardHeader className="text-base font-semibold capitalize">
                {variant} Card
              </CardHeader>
              <CardContent className="text-on-surface-variant text-xs leading-5">
                Tonal card surface with optional semantic header/footer dividers
                via CardContext.
              </CardContent>
              <CardFooter>
                <Button variant="basic" className="h-8 text-xs">
                  Action
                </Button>
              </CardFooter>
            </Card>
          ))}
        </div>
      </SpecimenCard>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <SpecimenCard
          title="Sparkline & Circle Meter"
          description="Smooth sliding-window D3 sparklines and SVG stroke-dashoffset ring meters."
          controls={
            <Button
              variant="stroked"
              className="h-8 gap-1.5 px-3 text-xs"
              onClick={pushSample}
            >
              <RefreshRounded className="size-4" />
              <span>Push Live Sample</span>
            </Button>
          }
          code={`<Sparkline data={series} className="text-primary h-24 w-full" />`}
        >
          <div className="flex flex-col gap-4">
            <div className="bg-surface-container-lowest dark:bg-surface-container h-28 overflow-hidden rounded-2xl p-2">
              <Sparkline data={series} className="text-primary size-full" />
            </div>

            <div className="flex items-center gap-4">
              <div className="relative grid size-16 place-items-center">
                <CircleSVG className="text-surface-container-highest -rotate-90">
                  <Circle value={100} />
                </CircleSVG>
                <CircleSVG className="text-primary -rotate-90">
                  <Circle value={series[series.length - 1]} />
                </CircleSVG>
                <span className="text-on-surface font-mono text-xs font-semibold tabular-nums">
                  {series[series.length - 1]}%
                </span>
              </div>

              <div className="text-on-surface-variant text-xs leading-5">
                Latest sample rendered simultaneously in <code>Sparkline</code>{' '}
                and <code>Circle</code>.
              </div>
            </div>
          </div>
        </SpecimenCard>

        <SpecimenCard
          title="HighlightText, TextMarquee & Kbd"
          description="Search match emphasis, automatic hover/overflow marquee scrolling, and monospace shortcut keys."
        >
          <div className="flex flex-col gap-4">
            <SearchField
              variant="basic"
              value={highlightQuery}
              onValueChange={setHighlightQuery}
              placeholder="Highlight substring..."
              clearLabel="Clear"
            />

            <div className="bg-surface-container-lowest dark:bg-surface-container rounded-2xl p-3 text-sm">
              <HighlightText searchText={highlightQuery}>
                Tokyo-01 · Premium IEPL Relay · tokyo.nyanpasu.local
              </HighlightText>
            </div>

            <div className="bg-surface-container-lowest dark:bg-surface-container max-w-64 rounded-2xl px-3 py-2 text-xs">
              <TextMarquee>
                Very long node label that smoothly scrolls inside tight
                containers: Singapore-Premium-BGP-04 · 28ms
              </TextMarquee>
            </div>

            <div className="flex flex-wrap items-center gap-2 text-xs">
              <span className="text-on-surface-variant">Shortcuts:</span>
              <Kbd>Ctrl</Kbd>
              <Kbd>K</Kbd>
              <span className="text-on-surface-variant">/</span>
              <Kbd>⌘</Kbd>
              <Kbd>⇧</Kbd>
              <Kbd>P</Kbd>
            </div>
          </div>
        </SpecimenCard>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <SpecimenCard
          title="EmptyState"
          description="Calm, centered fallback surface for empty lists and filtered views."
          code={`<EmptyState message="No matching rules" description="Adjust your filter or import a ruleset." />`}
        >
          <EmptyState
            message="No active proxy providers"
            description="Import a remote subscription or create a local profile to start routing traffic."
            className="min-h-40"
          />
        </SpecimenCard>

        <SpecimenCard
          title="LazyImage & AnimatedItem"
          description="Progressive image reveal with tonal skeleton placeholder and spring-mounted list items."
          controls={
            <Button
              variant="stroked"
              className="h-8 gap-1.5 px-3 text-xs"
              onClick={() => setAnimatedKey((k) => k + 1)}
            >
              <BoltRounded className="size-4" />
              <span>Replay AnimatedItem</span>
            </Button>
          }
        >
          <div className="flex flex-col gap-4">
            <LazyImage
              src={INLINE_SVG_PREVIEW}
              alt="Inline vector preview"
              className="h-24 w-full rounded-2xl object-cover"
            />

            <AnimatedItem key={animatedKey}>
              <div className="bg-secondary-container text-on-secondary-container rounded-2xl px-4 py-3 text-xs font-medium">
                AnimatedItem entrance (#{animatedKey + 1})
              </div>
            </AnimatedItem>
          </div>
        </SpecimenCard>
      </div>
    </div>
  )
}
