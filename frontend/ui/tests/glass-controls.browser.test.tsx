import '@root/frontend/theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { page } from 'vitest/browser'
import { Button } from '@nyanpasu/ui/button'
import { GlassControlGroup } from '@nyanpasu/ui/glass-control-group'
import { SearchField } from '@nyanpasu/ui/search-field'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { cn } from '@nyanpasu/utils'

const GLASS_SURFACE = cn(
  'bg-surface-container-high',
  'supports-[backdrop-filter]:bg-surface-container-high/76 supports-[backdrop-filter]:backdrop-blur-xl',
  'motion-reduce:backdrop-blur-none',
  '[@media(prefers-reduced-transparency:reduce)]:bg-surface-container-high',
  'contrast-more:bg-surface-container-high',
  'border border-outline-variant/40',
  'shadow-lg shadow-shadow/15 dark:shadow-shadow/40',
)

test.for(['light', 'dark'] as const)(
  'glass controls share the M3E glass surface in %s theme',
  async (theme, { onTestFinished }) => {
    const rootElement = document.documentElement
    const wasDark = rootElement.classList.contains('dark')
    rootElement.classList.toggle('dark', theme === 'dark')

    const container = document.createElement('div')
    document.body.append(container)
    const root = createRoot(container)
    onTestFinished(() => {
      root.unmount()
      container.remove()
      rootElement.classList.toggle('dark', wasDark)
    })

    flushSync(() => {
      root.render(
        <div>
          <button
            data-testid="legacy-button"
            type="button"
            className={`text-on-surface-variant hover:bg-surface-container-highest relative grid h-11 w-11 cursor-pointer place-content-center overflow-hidden rounded-full p-0 text-sm font-medium transition-[background-color,color,shadow,filter] focus:outline-hidden ${GLASS_SURFACE}`}
          >
            <span>+</span>
          </button>
          <Button data-testid="glass-button" variant="glass" icon disableRipple>
            <span>+</span>
          </Button>

          <SearchField
            data-testid="legacy-search"
            value=""
            onValueChange={() => {}}
            placeholder="Search"
            clearLabel="Clear search"
            className={`h-11 rounded-full ${GLASS_SURFACE}`}
          />
          <SearchField
            data-testid="glass-search"
            variant="glass"
            value=""
            onValueChange={() => {}}
            placeholder="Search"
            clearLabel="Clear search"
          />

          <div
            data-testid="legacy-group"
            className={`text-on-surface-variant flex min-w-0 items-center gap-1 rounded-full p-1 [&>button]:h-9 ${GLASS_SURFACE}`}
          >
            <Button icon disableRipple data-testid="legacy-group-icon">
              +
            </Button>
            <Button
              disableRipple
              data-testid="legacy-group-trigger"
              className="flex h-9 w-28 min-w-0 shrink-0 items-center gap-1 rounded-full px-3"
            >
              Control
            </Button>
            <Button icon disabled data-testid="legacy-group-disabled">
              +
            </Button>
            <span data-slot="custom-control">Custom</span>
          </div>
          <GlassControlGroup data-testid="glass-group">
            <Button
              variant="glass"
              icon
              disableRipple
              data-testid="glass-group-icon"
            >
              +
            </Button>
            <Button
              variant="glass"
              disableRipple
              data-testid="glass-group-trigger"
              className="w-28"
            >
              Control
            </Button>
            <Button
              variant="glass"
              icon
              disabled
              data-testid="glass-group-disabled"
            >
              +
            </Button>
            <span data-slot="custom-control">Custom</span>
          </GlassControlGroup>

          <SegmentedButton
            data-testid="legacy-segments"
            variant="tabs"
            size="sm"
            defaultValue="first"
            className="h-9 w-auto shrink-0 rounded-full bg-transparent **:data-[slot=segmented-button-item-state-layer]:rounded-full *:data-[slot=segmented-button-slider-pill]:rounded-full"
          >
            <SegmentedButtonItem
              value="first"
              data-testid="legacy-segment"
              className="flex-none rounded-full whitespace-nowrap"
            >
              First
            </SegmentedButtonItem>
            <SegmentedButtonItem
              value="second"
              className="flex-none rounded-full whitespace-nowrap"
            >
              Second
            </SegmentedButtonItem>
          </SegmentedButton>
          <SegmentedButton
            data-testid="glass-segments"
            variant="glass"
            size="sm"
            defaultValue="first"
          >
            <SegmentedButtonItem value="first" data-testid="glass-segment">
              First
            </SegmentedButtonItem>
            <SegmentedButtonItem value="second">Second</SegmentedButtonItem>
          </SegmentedButton>
        </div>,
      )
    })

    const element = (testId: string) =>
      container.querySelector<HTMLElement>(`[data-testid="${testId}"]`)!
    const styles = (testId: string) => getComputedStyle(element(testId))
    const geometry = (testId: string) => {
      const { width, height } = element(testId).getBoundingClientRect()
      return { width, height }
    }

    for (const [legacy, glass] of [
      ['legacy-button', 'glass-button'],
      ['legacy-search', 'glass-search'],
      ['legacy-group', 'glass-group'],
      ['legacy-group-icon', 'glass-group-icon'],
      ['legacy-group-trigger', 'glass-group-trigger'],
      ['legacy-group-disabled', 'glass-group-disabled'],
      ['legacy-segments', 'glass-segments'],
      ['legacy-segment', 'glass-segment'],
    ]) {
      const oldStyle = styles(legacy)
      const newStyle = styles(glass)
      expect(newStyle.backgroundColor).toBe(oldStyle.backgroundColor)
      expect(newStyle.color).toBe(oldStyle.color)
      expect(newStyle.boxShadow).toBe(oldStyle.boxShadow)
      expect(newStyle.borderRadius).toBe(oldStyle.borderRadius)
      expect(newStyle.transitionProperty).toBe(oldStyle.transitionProperty)
      expect(geometry(glass)).toEqual(geometry(legacy))
    }

    const legacyChild = element('legacy-group').querySelector('button')!
    const glassChild = element('glass-group').querySelector('button')!
    expect(glassChild.getBoundingClientRect().height).toBe(
      legacyChild.getBoundingClientRect().height,
    )

    const buttonAppearance = (testId: string) => {
      const style = styles(testId)
      return {
        color: style.color,
        backgroundColor: style.backgroundColor,
        boxShadow: style.boxShadow,
        outline: style.outline,
      }
    }

    // Compare interaction states at their settled values rather than at
    // different points in each button's color/shadow transition.
    for (const testId of ['legacy-button', 'glass-button']) {
      element(testId).style.transition = 'none'
    }

    element('legacy-button').focus()
    const focused = buttonAppearance('legacy-button')
    element('glass-button').focus()
    expect(buttonAppearance('glass-button')).toEqual(focused)
    element('glass-button').blur()

    await page.getByTestId('legacy-button').hover()
    const hovered = buttonAppearance('legacy-button')
    await page.getByTestId('glass-button').hover()
    await expect.poll(() => buttonAppearance('glass-button')).toEqual(hovered)
  },
)
