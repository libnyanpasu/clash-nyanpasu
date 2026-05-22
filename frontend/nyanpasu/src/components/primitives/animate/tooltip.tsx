import {
  AnimatePresence,
  LayoutGroup,
  motion,
  type HTMLMotionProps,
  type Transition,
} from 'framer-motion'
import {
  useCallback,
  useEffect,
  useId,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ComponentProps,
  type Dispatch,
  type FocusEvent,
  type MouseEvent,
  type PointerEvent,
  type ReactNode,
  type RefObject,
  type SetStateAction,
} from 'react'
import { getStrictContext } from '@/utils/get-strict-context'
import {
  autoUpdate,
  flip,
  arrow as floatingArrow,
  FloatingArrow,
  offset as floatingOffset,
  FloatingPortal,
  shift,
  useFloating,
  type UseFloatingReturn,
} from '@floating-ui/react'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { Slot, type WithAsChild } from './slot'

type Side = 'top' | 'bottom' | 'left' | 'right'
type Align = 'start' | 'center' | 'end'

type TooltipData = {
  contentProps: HTMLMotionProps<'div'>
  contentAsChild: boolean
  rect: DOMRect
  side: Side
  sideOffset: number
  align: Align
  alignOffset: number
  id: string
}

type GlobalTooltipContextType = {
  showTooltip: (data: TooltipData) => void
  hideTooltip: () => void
  hideImmediate: () => void
  currentTooltip: TooltipData | null
  transition: Transition
  globalId: string
  setReferenceEl: (el: HTMLElement | null) => void
  referenceElRef: RefObject<HTMLElement | null>
}

const [GlobalTooltipProvider, useGlobalTooltip] =
  getStrictContext<GlobalTooltipContextType>('GlobalTooltipProvider')

type PositionOverride = {
  side?: Side
  sideOffset?: number
  align?: Align
  alignOffset?: number
}

type TooltipContextType = {
  props: HTMLMotionProps<'div'>
  setProps: Dispatch<SetStateAction<HTMLMotionProps<'div'>>>
  asChild: boolean
  setAsChild: Dispatch<SetStateAction<boolean>>
  side: Side
  sideOffset: number
  align: Align
  alignOffset: number
  setPositionOverride: Dispatch<SetStateAction<PositionOverride>>
  id: string
  open: boolean
  setOpen: (open: boolean) => void
  isControlled: boolean
}

const [LocalTooltipProvider, useTooltip] = getStrictContext<TooltipContextType>(
  'LocalTooltipProvider',
)

type TooltipPosition = { x: number; y: number }

function getResolvedSide(placement: Side | `${Side}-${Align}`) {
  if (placement.includes('-')) {
    return placement.split('-')[0] as Side
  }

  return placement as Side
}

function initialFromSide(side: Side): Partial<Record<'x' | 'y', number>> {
  if (side === 'top') {
    return { y: 15 }
  }

  if (side === 'bottom') {
    return { y: -15 }
  }

  if (side === 'left') {
    return { x: 15 }
  }

  return { x: -15 }
}

type TooltipProviderProps = {
  children: ReactNode
  id?: string
  openDelay?: number
  closeDelay?: number
  transition?: Transition
}

function TooltipProvider({
  children,
  id,
  openDelay = 700,
  closeDelay = 300,
  transition = { type: 'spring', stiffness: 300, damping: 35 },
}: TooltipProviderProps) {
  const globalId = useId()
  const [currentTooltip, setCurrentTooltip] = useState<TooltipData | null>(null)
  const timeoutRef = useRef<number | null>(null)
  const lastCloseTimeRef = useRef<number>(0)
  const referenceElRef = useRef<HTMLElement | null>(null)

  const showTooltip = useCallback(
    (data: TooltipData) => {
      if (timeoutRef.current) clearTimeout(timeoutRef.current)
      if (currentTooltip !== null) {
        setCurrentTooltip(data)
        return
      }
      const now = Date.now()
      const delay = now - lastCloseTimeRef.current < closeDelay ? 0 : openDelay
      timeoutRef.current = window.setTimeout(
        () => setCurrentTooltip(data),
        delay,
      )
    },
    [openDelay, closeDelay, currentTooltip],
  )

  const hideTooltip = useCallback(() => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current)
    timeoutRef.current = window.setTimeout(() => {
      setCurrentTooltip(null)
      lastCloseTimeRef.current = Date.now()
    }, closeDelay)
  }, [closeDelay])

  const hideImmediate = useCallback(() => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current)
    setCurrentTooltip(null)
    lastCloseTimeRef.current = Date.now()
  }, [])

  const setReferenceEl = useCallback((el: HTMLElement | null) => {
    referenceElRef.current = el
  }, [])

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') hideImmediate()
    }
    window.addEventListener('keydown', onKeyDown, true)
    window.addEventListener('scroll', hideImmediate, true)
    window.addEventListener('resize', hideImmediate, true)
    return () => {
      window.removeEventListener('keydown', onKeyDown, true)
      window.removeEventListener('scroll', hideImmediate, true)
      window.removeEventListener('resize', hideImmediate, true)
    }
  }, [hideImmediate])

  return (
    <GlobalTooltipProvider
      value={{
        showTooltip,
        hideTooltip,
        hideImmediate,
        currentTooltip,
        transition,
        globalId: id ?? globalId,
        setReferenceEl,
        referenceElRef,
      }}
    >
      <LayoutGroup>{children}</LayoutGroup>
      <TooltipOverlay />
    </GlobalTooltipProvider>
  )
}

type RenderedTooltipContextType = {
  side: Side
  align: Align
  open: boolean
}

const [RenderedTooltipProvider, useRenderedTooltip] =
  getStrictContext<RenderedTooltipContextType>('RenderedTooltipContext')

type FloatingContextType = {
  context: UseFloatingReturn['context']
  arrowRef: RefObject<SVGSVGElement | null>
}

const [FloatingProvider, useFloatingContext] =
  getStrictContext<FloatingContextType>('FloatingContext')

const MotionTooltipArrow = motion.create(FloatingArrow)

type TooltipArrowProps = Omit<
  ComponentProps<typeof MotionTooltipArrow>,
  'context'
> & {
  withTransition?: boolean
}

function TooltipArrow({
  ref,
  withTransition = true,
  ...props
}: TooltipArrowProps) {
  const { side, align, open } = useRenderedTooltip()
  const { context, arrowRef } = useFloatingContext()
  const { transition, globalId } = useGlobalTooltip()
  useImperativeHandle(ref, () => arrowRef.current as SVGSVGElement)

  const deg = { top: 0, right: 90, bottom: 180, left: -90 }[side]

  return (
    <MotionTooltipArrow
      ref={arrowRef}
      context={context}
      data-state={open ? 'open' : 'closed'}
      data-side={side}
      data-align={align}
      data-slot="tooltip-arrow"
      style={{ rotate: deg }}
      layoutId={withTransition ? `tooltip-arrow-${globalId}` : undefined}
      transition={withTransition ? transition : undefined}
      {...props}
    />
  )
}

type TooltipPortalProps = ComponentProps<typeof FloatingPortal>

function TooltipPortal(props: TooltipPortalProps) {
  return <FloatingPortal {...props} />
}

function TooltipOverlay() {
  const { currentTooltip, transition, globalId, referenceElRef } =
    useGlobalTooltip()

  const [rendered, setRendered] = useState<{
    data: TooltipData | null
    open: boolean
  }>({ data: null, open: false })

  const arrowRef = useRef<SVGSVGElement | null>(null)

  const side = rendered.data?.side ?? 'top'
  const align = rendered.data?.align ?? 'center'

  const { refs, x, y, strategy, context, update } = useFloating({
    placement: align === 'center' ? side : `${side}-${align}`,
    whileElementsMounted: autoUpdate,
    middleware: [
      floatingOffset({
        mainAxis: rendered.data?.sideOffset ?? 0,
        crossAxis: rendered.data?.alignOffset ?? 0,
      }),
      flip(),
      shift({ padding: 8 }),
      floatingArrow({ element: arrowRef }),
    ],
  })

  useEffect(() => {
    if (currentTooltip) {
      setRendered({ data: currentTooltip, open: true })
    } else {
      setRendered((p) => (p.data ? { ...p, open: false } : p))
    }
  }, [currentTooltip])

  useLayoutEffect(() => {
    if (referenceElRef.current) {
      refs.setReference(referenceElRef.current)
      update()
    }
  }, [referenceElRef, refs, update, rendered.data])

  const ready = x != null && y != null
  const Component = rendered.data?.contentAsChild ? Slot : motion.div
  const resolvedSide = getResolvedSide(context.placement)

  return (
    <AnimatePresence mode="wait">
      {rendered.data && ready && (
        <TooltipPortal>
          <div
            ref={refs.setFloating}
            data-slot="tooltip-overlay"
            data-side={resolvedSide}
            data-align={rendered.data.align}
            data-state={rendered.open ? 'open' : 'closed'}
            style={{
              position: strategy,
              top: 0,
              left: 0,
              zIndex: 50,
              transform: `translate3d(${x!}px, ${y!}px, 0)`,
            }}
          >
            <FloatingProvider value={{ context, arrowRef }}>
              <RenderedTooltipProvider
                value={{
                  side: resolvedSide,
                  align: rendered.data.align,
                  open: rendered.open,
                }}
              >
                <Component
                  data-slot="tooltip-content"
                  data-side={resolvedSide}
                  data-align={rendered.data.align}
                  data-state={rendered.open ? 'open' : 'closed'}
                  layoutId={`tooltip-content-${globalId}`}
                  initial={{
                    opacity: 0,
                    scale: 0,
                    ...initialFromSide(rendered.data.side),
                  }}
                  animate={
                    rendered.open
                      ? { opacity: 1, scale: 1, x: 0, y: 0 }
                      : {
                          opacity: 0,
                          scale: 0,
                          ...initialFromSide(rendered.data.side),
                        }
                  }
                  exit={{
                    opacity: 0,
                    scale: 0,
                    ...initialFromSide(rendered.data.side),
                  }}
                  onAnimationComplete={() => {
                    if (!rendered.open) setRendered({ data: null, open: false })
                  }}
                  transition={transition}
                  {...rendered.data.contentProps}
                  style={{
                    position: 'relative',
                    ...(rendered.data.contentProps?.style || {}),
                  }}
                />
              </RenderedTooltipProvider>
            </FloatingProvider>
          </div>
        </TooltipPortal>
      )}
    </AnimatePresence>
  )
}

type TooltipProps = {
  children: ReactNode
  side?: Side
  sideOffset?: number
  align?: Align
  alignOffset?: number
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
}

function Tooltip({
  children,
  side: sideProp = 'top',
  sideOffset: sideOffsetProp = 0,
  align: alignProp = 'center',
  alignOffset: alignOffsetProp = 0,
  open: openProp,
  defaultOpen = false,
  onOpenChange,
}: TooltipProps) {
  const id = useId()
  const [props, setProps] = useState<HTMLMotionProps<'div'>>({})
  const [asChild, setAsChild] = useState(false)
  const [positionOverride, setPositionOverride] = useState<PositionOverride>({})

  const [open = false, setOpen] = useControllableState<boolean>({
    prop: openProp,
    defaultProp: defaultOpen,
    onChange: onOpenChange,
  })

  const side = positionOverride.side ?? sideProp
  const sideOffset = positionOverride.sideOffset ?? sideOffsetProp
  const align = positionOverride.align ?? alignProp
  const alignOffset = positionOverride.alignOffset ?? alignOffsetProp

  const isControlled = openProp !== undefined

  const contextValue = useMemo(
    () => ({
      props,
      setProps,
      asChild,
      setAsChild,
      side,
      sideOffset,
      align,
      alignOffset,
      setPositionOverride,
      id,
      open,
      setOpen,
      isControlled,
    }),
    [
      props,
      asChild,
      side,
      sideOffset,
      align,
      alignOffset,
      id,
      open,
      setOpen,
      isControlled,
    ],
  )

  return (
    <LocalTooltipProvider value={contextValue}>{children}</LocalTooltipProvider>
  )
}

type TooltipContentProps = WithAsChild<HTMLMotionProps<'div'>> & {
  side?: Side
  sideOffset?: number
  align?: Align
  alignOffset?: number
}

function shallowEqualWithoutChildren(
  a?: HTMLMotionProps<'div'>,
  b?: HTMLMotionProps<'div'>,
) {
  if (a === b) return true
  if (!a || !b) return false
  const keysA = Object.keys(a).filter((k) => k !== 'children')
  const keysB = Object.keys(b).filter((k) => k !== 'children')
  if (keysA.length !== keysB.length) return false
  for (const k of keysA) {
    // @ts-expect-error index
    if (a[k] !== b[k]) return false
  }
  return true
}

function TooltipContent({
  asChild = false,
  side,
  sideOffset,
  align,
  alignOffset,
  ...props
}: TooltipContentProps) {
  const { setProps, setAsChild, setPositionOverride } = useTooltip()
  const lastPropsRef = useRef<HTMLMotionProps<'div'> | undefined>(undefined)

  useEffect(() => {
    if (!shallowEqualWithoutChildren(lastPropsRef.current, props)) {
      lastPropsRef.current = props
      setProps(props)
    }
  }, [props, setProps])

  useEffect(() => {
    setAsChild(asChild)
  }, [asChild, setAsChild])

  useEffect(() => {
    setPositionOverride({ side, sideOffset, align, alignOffset })
  }, [side, sideOffset, align, alignOffset, setPositionOverride])

  return null
}

type TooltipTriggerProps = WithAsChild<HTMLMotionProps<'div'>>

function TooltipTrigger({
  ref,
  onMouseEnter,
  onMouseLeave,
  onFocus,
  onBlur,
  onPointerDown,
  asChild = false,
  ...props
}: TooltipTriggerProps) {
  const {
    props: contentProps,
    asChild: contentAsChild,
    side,
    sideOffset,
    align,
    alignOffset,
    id,
    open,
    setOpen,
    isControlled,
  } = useTooltip()
  const {
    showTooltip,
    hideTooltip,
    hideImmediate,
    currentTooltip,
    setReferenceEl,
  } = useGlobalTooltip()

  const triggerRef = useRef<HTMLDivElement>(null)
  useImperativeHandle(ref, () => triggerRef.current as HTMLDivElement)

  const suppressNextFocusRef = useRef(false)

  const handleOpen = useCallback(() => {
    if (!triggerRef.current) return
    setReferenceEl(triggerRef.current)
    const rect = triggerRef.current.getBoundingClientRect()
    showTooltip({
      contentProps,
      contentAsChild,
      rect,
      side,
      sideOffset,
      align,
      alignOffset,
      id,
    })
  }, [
    showTooltip,
    setReferenceEl,
    contentProps,
    contentAsChild,
    side,
    sideOffset,
    align,
    alignOffset,
    id,
  ])

  useEffect(() => {
    if (!isControlled) return
    const isShown = currentTooltip?.id === id
    if (open && !isShown) {
      handleOpen()
    } else if (!open && isShown) {
      hideImmediate()
    }
  }, [open, isControlled, currentTooltip?.id, id, handleOpen, hideImmediate])

  const handlePointerDown = useCallback(
    (e: PointerEvent<HTMLDivElement>) => {
      onPointerDown?.(e)
      if (currentTooltip?.id === id) {
        suppressNextFocusRef.current = true
        hideImmediate()
        if (!isControlled) setOpen(false)
        Promise.resolve().then(() => {
          suppressNextFocusRef.current = false
        })
      }
    },
    [
      onPointerDown,
      currentTooltip?.id,
      id,
      hideImmediate,
      isControlled,
      setOpen,
    ],
  )

  const handleMouseEnter = useCallback(
    (e: MouseEvent<HTMLDivElement>) => {
      onMouseEnter?.(e)
      if (isControlled) {
        if (open) handleOpen()
        return
      }
      handleOpen()
      setOpen(true)
    },
    [handleOpen, onMouseEnter, isControlled, open, setOpen],
  )

  const handleMouseLeave = useCallback(
    (e: MouseEvent<HTMLDivElement>) => {
      onMouseLeave?.(e)
      if (isControlled) {
        if (!open) hideTooltip()
        return
      }
      hideTooltip()
      setOpen(false)
    },
    [hideTooltip, onMouseLeave, isControlled, open, setOpen],
  )

  const handleFocus = useCallback(
    (e: FocusEvent<HTMLDivElement>) => {
      onFocus?.(e)
      if (suppressNextFocusRef.current) return
      if (isControlled) {
        if (open) handleOpen()
        return
      }
      handleOpen()
      setOpen(true)
    },
    [handleOpen, onFocus, isControlled, open, setOpen],
  )

  const handleBlur = useCallback(
    (e: FocusEvent<HTMLDivElement>) => {
      onBlur?.(e)
      if (isControlled) {
        if (!open) hideTooltip()
        return
      }
      hideTooltip()
      setOpen(false)
    },
    [hideTooltip, onBlur, isControlled, open, setOpen],
  )

  const Component = asChild ? Slot : motion.div

  return (
    <Component
      ref={triggerRef}
      onPointerDown={handlePointerDown}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      onFocus={handleFocus}
      onBlur={handleBlur}
      data-slot="tooltip-trigger"
      data-side={side}
      data-align={align}
      data-state={currentTooltip?.id === id ? 'open' : 'closed'}
      {...props}
    />
  )
}

export {
  TooltipProvider,
  Tooltip,
  TooltipContent,
  TooltipTrigger,
  TooltipArrow,
  useGlobalTooltip,
  useTooltip,
  type TooltipProviderProps,
  type TooltipProps,
  type TooltipContentProps,
  type TooltipTriggerProps,
  type TooltipArrowProps,
  type TooltipPosition,
  type GlobalTooltipContextType,
  type TooltipContextType,
}
