import { AnimatePresence } from 'motion/react'
import { Slot } from 'radix-ui'
import { DismissableLayer, Popper } from 'radix-ui/internal'
import {
  createContext,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type FocusEvent,
  type PointerEvent,
  type ReactElement,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'
import { TooltipBubble, type TooltipContentProps } from './tooltip'

// Radix tooltips announce that one opens with this event, and close on it.
const TOOLTIP_OPEN = 'tooltip.open'

type Point = { x: number; y: number }

type Shown = { key: number; trigger: HTMLElement; content: ReactNode }

type SharedTooltipActions = {
  enter: (trigger: HTMLElement, content: ReactNode) => void
  leave: (event: PointerEvent<HTMLElement>) => void
  press: () => void
  focus: (trigger: HTMLElement, content: ReactNode) => void
  close: () => void
}

const SharedTooltipContext = createContext<SharedTooltipActions | null>(null)

export type SharedTooltipProps = Pick<
  TooltipContentProps,
  'side' | 'sideOffset' | 'className'
> & {
  children: ReactNode
  /** As `TooltipProvider`'s, which a shared tooltip does not read. */
  openDelay?: number
  closeDelay?: number
}

/**
 * One tooltip for many triggers, such as the rows of a long list. A `Tooltip`
 * per row mounts a popper, its state and its context with every row; here a
 * row carries only the handlers of `useSharedTooltipTrigger`, and a tooltip
 * shows for the row that is hovered or focused.
 *
 * It behaves as `Tooltip` does: it opens after a delay on hover, at once
 * after another tooltip just closed and on keyboard focus, stays open while
 * the pointer moves onto it, closes on press, click, blur, Escape and on
 * scrolling the trigger, and describes its trigger while open. Render it
 * inside the dialog its triggers are in, so that pressing the tooltip does
 * not count as pressing outside the dialog.
 */
export function SharedTooltip({
  children,
  side = 'top',
  sideOffset = 8,
  className,
  openDelay = 120,
  closeDelay = 300,
}: SharedTooltipProps) {
  const id = useId()
  const [shown, setShown] = useState<Shown | null>(null)
  const [graceArea, setGraceArea] = useState<Point[] | null>(null)

  // Read by `enter`: a trigger the pointer crosses on its way to the open
  // tooltip does not open its own.
  const graceAreaRef = useRef(graceArea)
  graceAreaRef.current = graceArea

  const delays = useRef({ openDelay, closeDelay })
  delays.current = { openDelay, closeDelay }
  const state = useRef({
    shown: null as Shown | null,
    content: null as { key: number; node: HTMLElement } | null,
    // The trigger the pointer is over, whether or not its tooltip opened.
    entered: null as HTMLElement | null,
    openTimer: 0,
    skipTimer: 0,
    // Whether a tooltip closed too recently to wait for the delay again.
    instant: false,
    pointerDown: false,
    announcing: false,
    keys: 0,
  })

  const actions = useMemo<SharedTooltipActions>(() => {
    const current = state.current

    const show = (trigger: HTMLElement, content: ReactNode) => {
      window.clearTimeout(current.openTimer)
      if (current.shown?.trigger === trigger) {
        return
      }
      current.announcing = true
      document.dispatchEvent(new CustomEvent(TOOLTIP_OPEN))
      current.announcing = false
      current.shown = { key: (current.keys += 1), trigger, content }
      setShown(current.shown)
      window.clearTimeout(current.skipTimer)
      current.instant = true
    }

    const close = () => {
      window.clearTimeout(current.openTimer)
      if (!current.shown) {
        return
      }
      current.shown = null
      setShown(null)
      setGraceArea(null)
      window.clearTimeout(current.skipTimer)
      current.skipTimer = window.setTimeout(() => {
        current.instant = false
      }, delays.current.closeDelay)
    }

    return {
      enter: (trigger, content) => {
        if (current.entered === trigger || graceAreaRef.current) {
          return
        }
        current.entered = trigger
        window.clearTimeout(current.openTimer)
        if (current.instant) {
          show(trigger, content)
        } else {
          current.openTimer = window.setTimeout(
            () => show(trigger, content),
            delays.current.openDelay,
          )
        }
      },
      leave: (event) => {
        current.entered = null
        window.clearTimeout(current.openTimer)
        const content = current.content?.node
        if (current.shown?.trigger === event.currentTarget && content) {
          setGraceArea(exitGraceArea(event, content))
        }
      },
      press: () => {
        close()
        current.pointerDown = true
        document.addEventListener(
          'pointerup',
          () => {
            current.pointerDown = false
          },
          { once: true },
        )
      },
      focus: (trigger, content) => {
        if (!current.pointerDown) {
          show(trigger, content)
        }
      },
      close,
    }
  }, [])

  useEffect(() => {
    if (!graceArea) {
      return
    }

    const track = (event: globalThis.PointerEvent) => {
      const { shown, content } = state.current
      const target = event.target as Node
      if (shown?.trigger.contains(target) || content?.node.contains(target)) {
        setGraceArea(null)
      } else if (
        !isPointInPolygon({ x: event.clientX, y: event.clientY }, graceArea)
      ) {
        setGraceArea(null)
        actions.close()
      }
    }
    document.addEventListener('pointermove', track)
    return () => document.removeEventListener('pointermove', track)
  }, [graceArea, actions])

  useEffect(() => {
    if (!shown) {
      return
    }

    const closeOnOther = () => {
      if (!state.current.announcing) actions.close()
    }
    const closeOnScroll = (event: Event) => {
      if (
        event.target instanceof Node &&
        event.target.contains(shown.trigger)
      ) {
        actions.close()
      }
    }
    document.addEventListener(TOOLTIP_OPEN, closeOnOther)
    window.addEventListener('scroll', closeOnScroll, { capture: true })
    return () => {
      document.removeEventListener(TOOLTIP_OPEN, closeOnOther)
      window.removeEventListener('scroll', closeOnScroll, { capture: true })
    }
  }, [shown, actions])

  // The trigger is described by its tooltip while it shows.
  useLayoutEffect(() => {
    if (!shown) {
      return
    }

    const { trigger, key } = shown
    const previous = trigger.getAttribute('aria-describedby')
    trigger.setAttribute(
      'aria-describedby',
      [previous, `${id}-${key}`].filter(Boolean).join(' '),
    )
    return () => {
      if (previous === null) {
        trigger.removeAttribute('aria-describedby')
      } else {
        trigger.setAttribute('aria-describedby', previous)
      }
    }
  }, [shown, id])

  useEffect(() => {
    const current = state.current
    return () => {
      window.clearTimeout(current.openTimer)
      window.clearTimeout(current.skipTimer)
    }
  }, [])

  const handleClose = actions.close

  return (
    <SharedTooltipContext value={actions}>
      {children}

      {createPortal(
        <AnimatePresence>
          {shown && (
            <SharedTooltipContent
              key={shown.key}
              id={`${id}-${shown.key}`}
              shown={shown}
              side={side}
              sideOffset={sideOffset}
              className={className}
              onContent={(node) => {
                const { content } = state.current
                if (node) {
                  state.current.content = { key: shown.key, node }
                } else if (content?.key === shown.key) {
                  state.current.content = null
                }
              }}
              onClose={handleClose}
              onPointerLeave={(event) => {
                const { shown: current } = state.current
                if (current?.key === shown.key) {
                  setGraceArea(exitGraceArea(event, current.trigger))
                }
              }}
            />
          )}
        </AnimatePresence>,
        document.body,
      )}
    </SharedTooltipContext>
  )
}

function SharedTooltipContent({
  id,
  shown,
  side,
  sideOffset,
  className,
  onContent,
  onClose,
  onPointerLeave,
}: {
  id: string
  shown: Shown
  side: NonNullable<TooltipContentProps['side']>
  sideOffset: number
  className?: string
  onContent: (node: HTMLElement | null) => void
  onClose: () => void
  onPointerLeave: (event: PointerEvent<HTMLElement>) => void
}) {
  // Fixed for this showing: a tooltip easing out stays at its own trigger.
  const anchor = useMemo(() => ({ current: shown.trigger }), [shown.trigger])

  return (
    <Popper.Root>
      <Popper.Anchor virtualRef={anchor} />

      <DismissableLayer.Root
        asChild
        disableOutsidePointerEvents={false}
        onFocusOutside={(event) => event.preventDefault()}
        onDismiss={onClose}
      >
        <Popper.Content
          ref={onContent}
          role="tooltip"
          id={id}
          side={side}
          sideOffset={sideOffset}
          className="z-50"
          onPointerLeave={onPointerLeave}
        >
          <TooltipBubble side={side} className={className}>
            {shown.content}
          </TooltipBubble>
        </Popper.Content>
      </DismissableLayer.Root>
    </Popper.Root>
  )
}

/**
 * The props that make an element a trigger of the enclosing `SharedTooltip`,
 * showing `content`. Compose them with the element's own handlers.
 */
export function useSharedTooltipTrigger(content: ReactNode) {
  const actions = useContext(SharedTooltipContext)

  if (actions === null) {
    throw new Error('useSharedTooltipTrigger must be used within SharedTooltip')
  }

  return {
    onPointerMove: (event: PointerEvent<HTMLElement>) => {
      if (event.pointerType !== 'touch') {
        actions.enter(event.currentTarget, content)
      }
    },
    onPointerLeave: actions.leave,
    onPointerDown: actions.press,
    onFocus: (event: FocusEvent<HTMLElement>) =>
      actions.focus(event.currentTarget, content),
    onBlur: actions.close,
    onClick: actions.close,
  }
}

/**
 * Makes its only child a trigger of the enclosing `SharedTooltip`, showing
 * `content`, as `TooltipTrigger asChild` does for a `Tooltip`. The child's own
 * handlers run first.
 */
export function SharedTooltipTrigger({
  content,
  children,
}: {
  content: ReactNode
  children: ReactElement
}) {
  return <Slot.Root {...useSharedTooltipTrigger(content)}>{children}</Slot.Root>
}

// The pointer's way from where it left one element to another, as Radix's
// hoverable tooltip content computes it.
function exitGraceArea(event: PointerEvent<HTMLElement>, target: HTMLElement) {
  const exit = { x: event.clientX, y: event.clientY }
  const rect = event.currentTarget.getBoundingClientRect()
  const padding = 5
  const distances = {
    top: Math.abs(rect.top - exit.y),
    bottom: Math.abs(rect.bottom - exit.y),
    left: Math.abs(rect.left - exit.x),
    right: Math.abs(rect.right - exit.x),
  }
  const side = (Object.keys(distances) as (keyof typeof distances)[]).reduce(
    (nearest, next) => (distances[next] < distances[nearest] ? next : nearest),
  )
  const inward = {
    top: { x: 0, y: padding },
    bottom: { x: 0, y: -padding },
    left: { x: padding, y: 0 },
    right: { x: -padding, y: 0 },
  }[side]
  const across = side === 'top' || side === 'bottom'
  const exitPoints = [-padding, padding].map((spread) => ({
    x: exit.x + inward.x + (across ? spread : 0),
    y: exit.y + inward.y + (across ? 0 : spread),
  }))
  const { top, right, bottom, left } = target.getBoundingClientRect()

  return convexHull([
    ...exitPoints,
    { x: left, y: top },
    { x: right, y: top },
    { x: right, y: bottom },
    { x: left, y: bottom },
  ])
}

function isPointInPolygon({ x, y }: Point, polygon: Point[]) {
  let inside = false
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const a = polygon[i]
    const b = polygon[j]
    if (
      a.y > y !== b.y > y &&
      x < ((b.x - a.x) * (y - a.y)) / (b.y - a.y) + a.x
    ) {
      inside = !inside
    }
  }
  return inside
}

// Andrew's monotone chain.
function convexHull(points: Point[]) {
  const sorted = [...points].sort((a, b) => a.x - b.x || a.y - b.y)
  const cross = (o: Point, a: Point, b: Point) =>
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
  const half = (list: Point[]) => {
    const hull: Point[] = []
    for (const point of list) {
      while (
        hull.length >= 2 &&
        cross(hull[hull.length - 2], hull[hull.length - 1], point) <= 0
      ) {
        hull.pop()
      }
      hull.push(point)
    }
    hull.pop()
    return hull
  }

  return [...half(sorted), ...half([...sorted].reverse())]
}
