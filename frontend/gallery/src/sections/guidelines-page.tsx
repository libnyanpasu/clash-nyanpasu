import ArrowForwardRounded from '~icons/material-symbols/arrow-forward-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import ShareRounded from '~icons/material-symbols/share-rounded'
import { Button } from '@nyanpasu/ui'
import { GuidelineComparison, PageHeader } from '../studio/stage'

export function GuidelinesPage() {
  return (
    <div className="flex flex-col gap-6" data-slot="gallery-guidelines-page">
      <PageHeader
        eyebrow="Guidelines · Living Spec"
        title="Do & Don’t Visual Contracts"
        description="Every rule from docs/development/design-language.md and the m3-expressive-ui skill rendered live with real tokens, so design reviews compare tangible artifacts instead of abstract prose."
        badges={['design-language.md', 'm3-expressive-ui', 'WCAG AA']}
      />

      <GuidelineComparison
        title="1. Brand-Tinted Chrome & Semantic Container Pairs vs. Dark-Mode Role Inversion"
        rule="Pair primary-container (WindowHeader) with secondary-container (AppNavbar / EditorToolbar) and primary-container active navbar pills using their matching on-* foreground tokens. Never invert primary/on-primary roles across light and dark modes."
        doTitle="Semantic primary / secondary container chrome"
        doNote="Keeps Clash Nyanpasu's signature brand-tinted chrome while preserving WCAG contrast across all 9 MCU schemes."
        doDemo={
          <div className="bg-secondary-container text-on-secondary-container rounded-large flex items-center gap-2 p-2">
            <span className="bg-primary-container text-on-primary-container inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium">
              <CheckRounded className="size-4" />
              Proxies
            </span>
            <span className="text-on-secondary-container/80 hover:bg-on-secondary-container/10 rounded-full px-3.5 py-1.5 text-xs font-medium">
              Profiles
            </span>
          </div>
        }
        dontTitle="Inverted primary / on-primary hack"
        dontNote="Uses on-primary as a background in dark mode, breaking contrast on high-contrast or monochrome themes."
        dontDemo={
          <div className="bg-primary-container dark:bg-on-primary rounded-large flex items-center gap-2 p-2">
            <span className="bg-inverse-primary dark:bg-primary-container text-primary inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium">
              Proxies
            </span>
            <span className="bg-surface-variant text-on-surface-variant rounded-full px-3.5 py-1.5 text-xs font-medium">
              Profiles
            </span>
          </div>
        }
      />

      <GuidelineComparison
        title="2. Asymmetric Grouped Lists vs. Card Soup"
        rule="Group related settings rows using tonal surfaces and asymmetric corners (24px outer, 4px inner, 4px gap) rather than wrapping every row in bordered drop-shadow cards."
        doTitle="Tonal grouped list (no shadows)"
        doNote="Rows read as one cohesive group while clearly separating each touch target."
        doDemo={
          <div className="flex flex-col gap-1">
            <div className="bg-surface-container-lowest dark:bg-surface-container text-on-surface rounded-t-large rounded-b-extra-small px-4 py-2.5 text-xs font-medium">
              System Proxy
            </div>
            <div className="bg-surface-container-lowest dark:bg-surface-container text-on-surface rounded-extra-small px-4 py-2.5 text-xs font-medium">
              TUN Mode
            </div>
            <div className="bg-surface-container-lowest dark:bg-surface-container text-on-surface rounded-t-extra-small rounded-b-large px-4 py-2.5 text-xs font-medium">
              Allow LAN
            </div>
          </div>
        }
        dontTitle="Card soup with borders & shadows"
        dontNote="Every row competes for elevation, creating visual noise on dense settings screens."
        dontDemo={
          <div className="flex flex-col gap-3">
            {['System Proxy', 'TUN Mode', 'Allow LAN'].map((label) => (
              <div
                key={label}
                className="bg-surface-container-lowest dark:bg-surface-container border-outline text-on-surface rounded-medium border px-4 py-2.5 text-xs font-medium shadow-md"
              >
                {label}
              </div>
            ))}
          </div>
        }
      />

      <GuidelineComparison
        title="3. Single High-Emphasis CTA vs. Uniform Loudness"
        rule="Give each section one unmistakably primary CTA (flat or fab); keep secondary actions stroked or basic so attention lands where it matters."
        doTitle="Clear action hierarchy"
        doNote="Primary Save stands out immediately against calm secondary actions."
        doDemo={
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="flat">Apply</Button>
            <Button variant="stroked">Test</Button>
            <Button variant="basic">Cancel</Button>
          </div>
        }
        dontTitle="Every button screaming at once"
        dontNote="When every control uses filled primary weight, nothing has hierarchy."
        dontDemo={
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="flat">Apply</Button>
            <Button variant="flat">Test</Button>
            <Button variant="flat">Cancel</Button>
          </div>
        }
      />

      <GuidelineComparison
        title="4. Start-Aligned Menu Items vs. Split Justification"
        rule="Per docs/development/typescript.md, menu items align icon and label to the start. Only component-owned indicators (submenu arrows, shortcuts) sit at the trailing edge."
        doTitle="Start-aligned icon + label"
        doNote="Predictable left scanline across dropdowns, context menus, and menubars."
        doDemo={
          <div className="bg-surface-container-high rounded-large flex w-56 flex-col p-1.5">
            <div className="hover:bg-on-surface/8 text-on-surface rounded-medium flex items-center gap-2.5 px-3 py-2 text-xs">
              <EditRounded className="size-4" />
              <span>Edit profile</span>
            </div>
            <div className="hover:bg-on-surface/8 text-on-surface rounded-medium flex items-center gap-2.5 px-3 py-2 text-xs">
              <ShareRounded className="size-4" />
              <span>Share link</span>
            </div>
          </div>
        }
        dontTitle="justify-between splitting label"
        dontNote="Forces the eye to ping-pong horizontally across menus."
        dontDemo={
          <div className="bg-surface-container-high rounded-large flex w-56 flex-col p-1.5">
            <div className="text-on-surface rounded-medium flex items-center justify-between px-3 py-2 text-xs">
              <EditRounded className="size-4" />
              <span>Edit profile</span>
            </div>
            <div className="text-on-surface rounded-medium flex items-center justify-between px-3 py-2 text-xs">
              <ShareRounded className="size-4" />
              <span>Share link</span>
            </div>
          </div>
        }
      />

      <GuidelineComparison
        title="5. Single-Line Inline Button & Tab Layout vs. Wrapped Icon + Label"
        rule="Button, SegmentedButtonItem, and AnimatedTabsItem default to inline/flex single-line layout (items-center justify-center whitespace-nowrap) with non-shrinking icons so labels and icons stay on one line. Only opt into whitespace-normal for intentional multi-line controls."
        doTitle="Single-line inline-flex button"
        doNote="Label and icon align horizontally on one line without extra flex classes at every call site."
        doDemo={
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="flat">
              <span>Explore Color System</span>
              <ArrowForwardRounded className="size-4" />
            </Button>
            <Button variant="stroked">
              <span>Inspect surface tiers</span>
              <ArrowForwardRounded className="size-4" />
            </Button>
          </div>
        }
        dontTitle="Inline-block / wrapped button content"
        dontNote="Block-level SVG icons drop onto a second line and clip inside the fixed 40px pill height."
        dontDemo={
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="flat" className="inline-block whitespace-normal">
              <span>Explore Color System</span>
              <ArrowForwardRounded className="size-4" />
            </Button>
          </div>
        }
      />
    </div>
  )
}
