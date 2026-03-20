import CloseRounded from '~icons/material-symbols/close-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import {
  noCompactor,
  ResponsiveGridLayout,
  useContainerWidth,
  type Layout,
  type ResponsiveLayouts,
} from 'react-grid-layout'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { Button } from '@/components/ui/button'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import { DEFAULT_LAYOUTS, WIDGETS } from './_modules/consts'
import { useDashboardContext } from './_modules/provider'
import { WidgetSheet } from './_modules/widget-sheet'
import 'react-grid-layout/css/styles.css'
import HeaderAction from './_modules/header-action'

const WIDGET_MAP = Object.fromEntries(WIDGETS.map((w) => [w.id, w]))

const BREAKPOINTS = {
  lg: 1200,
  md: 996,
  sm: 768,
  xs: 480,
}
const COLS = {
  lg: 12,
  md: 8,
  sm: 6,
  xs: 4,
}
const MAX_ROWS = 12
const MARGIN: [number, number] = [16, 16]

const DEFAULT_ACTIVE = WIDGETS.map((w) => w.id)

export const Route = createFileRoute('/(main)/main/dashboard/')({
  component: RouteComponent,
})

function GridComponent() {
  const { width, containerRef } = useContainerWidth()

  const { isEditing, openSheet, setOpenSheet } = useDashboardContext()

  const [containerHeight, setContainerHeight] = useState(0)
  const suppressTransition = useRef(true)
  const [state, setState] = useState<{
    layouts: ResponsiveLayouts
    activeWidgets: string[]
  }>({
    layouts: DEFAULT_LAYOUTS,
    activeWidgets: DEFAULT_ACTIVE,
  })

  // Filter each breakpoint layout to only include active widgets
  const activeLayouts = useMemo<ResponsiveLayouts>(() => {
    const result: ResponsiveLayouts = {}
    for (const [bp, layout] of Object.entries(state.layouts)) {
      if (layout) {
        result[bp] = (layout as Layout).filter((item) =>
          state.activeWidgets.includes(item.i),
        )
      }
    }
    return result
  }, [state])

  const hiddenWidgets = useMemo(
    () => WIDGETS.filter((w) => !state.activeWidgets.includes(w.id)),
    [state.activeWidgets],
  )

  useLayoutEffect(() => {
    const containerEl = containerRef.current

    if (!containerEl) {
      return
    }

    setContainerHeight(containerEl.clientHeight)
  }, [containerRef])

  useEffect(() => {
    const raf = requestAnimationFrame(() => {
      suppressTransition.current = false
    })

    return () => cancelAnimationFrame(raf)
  }, [])

  useEffect(() => {
    const containerEl = containerRef.current

    if (!containerEl) {
      return
    }

    const observer = new ResizeObserver(() => {
      setContainerHeight(containerEl.clientHeight)
    })

    observer.observe(containerEl)

    return () => {
      observer.disconnect()
    }
  }, [containerRef])

  const rowHeight = useMemo(() => {
    if (containerHeight <= 0) {
      return 1
    }

    const gridHeight = Math.max(containerHeight - MARGIN[1] * 2, 1)
    const marginHeight = MARGIN[1] * Math.max(MAX_ROWS - 1, 0)

    return Math.max((gridHeight - marginHeight) / MAX_ROWS, 1)
  }, [containerHeight])

  const handleLayoutChange = useCallback(
    (_layout: Layout, allLayouts: ResponsiveLayouts) => {
      setState((prev) => ({ ...prev, layouts: allLayouts }))
    },
    [setState],
  )

  const handleRemove = useCallback(
    (id: string) => {
      setState((prev) => ({
        activeWidgets: prev.activeWidgets.filter((w) => w !== id),
        layouts: Object.fromEntries(
          Object.entries(prev.layouts).map(([bp, layout]) => [
            bp,
            layout ? (layout as Layout).filter((item) => item.i !== id) : [],
          ]),
        ),
      }))
    },
    [setState],
  )

  // const handleAdd = useCallback(
  //   (id: string) => {
  //     setState((prev) => {
  //       const newLayouts = Object.fromEntries(
  //         Object.entries(prev.layouts).map(([bp, layout]) => {
  //           const existing = (layout as Layout) ?? []
  //           const defaultBp = DEFAULT_LAYOUTS[bp] as Layout | undefined
  //           const defaultItem = defaultBp?.find((item) => item.i === id)
  //           return [bp, defaultItem ? [...existing, defaultItem] : existing]
  //         }),
  //       )
  //       return {
  //         activeWidgets: [...prev.activeWidgets, id],
  //         layouts: newLayouts,
  //       }
  //     })
  //   },
  //   [setState],
  // )

  // const handleReset = useCallback(() => {
  //   setState({ layouts: DEFAULT_LAYOUTS, activeWidgets: DEFAULT_ACTIVE })
  // }, [setState])

  return (
    <div
      ref={containerRef}
      data-edit={String(isEditing)}
      className={cn(
        'size-full rounded-3xl transition-[border]',
        'border-outline/30',
        'data-[edit=true]:border',
        'data-[edit=false]:border-0',
      )}
      data-slot="dashboard-grid"
    >
      <ResponsiveGridLayout
        className={cn(
          'size-full',
          suppressTransition.current && '[&_.react-grid-item]:transition-none!',
          '[&_.react-grid-placeholder]:bg-secondary/30!',
          '[&_.react-grid-placeholder]:border-2',
          '[&_.react-grid-placeholder]:border-outline!',
          '[&_.react-grid-placeholder]:rounded-3xl',
          isEditing && 'select-none',
        )}
        width={width}
        breakpoints={BREAKPOINTS}
        cols={COLS}
        layouts={activeLayouts}
        rowHeight={rowHeight}
        maxRows={MAX_ROWS}
        margin={MARGIN}
        autoSize={false}
        compactor={noCompactor}
        dragConfig={{
          enabled: isEditing,
          bounded: true,
        }}
        resizeConfig={{
          enabled: isEditing,
          handles: ['s', 'se', 'e'],
          // handleComponent: () => <div className="bg-primary/50 size-2" />,
        }}
        onLayoutChange={handleLayoutChange}
        onDrag={() => {
          // console.log('change')
        }}
      >
        {state.activeWidgets
          .filter((id) => WIDGET_MAP[id])
          .map((id) => {
            const { Component } = WIDGET_MAP[id]

            return (
              <div key={id} className="relative h-full w-full">
                <Component />

                {isEditing && (
                  <div className="ring-primary/50 absolute inset-0 z-10 cursor-move rounded-3xl ring-2">
                    <Button
                      icon
                      variant="raised"
                      className="bg-surface/80! absolute top-1 right-1 size-7 backdrop-blur-sm"
                      onClick={(e) => {
                        e.stopPropagation()
                        handleRemove(id)
                      }}
                    >
                      <CloseRounded className="size-4" />
                    </Button>
                  </div>
                )}
              </div>
            )
          })}
      </ResponsiveGridLayout>

      <WidgetSheet open={openSheet} onOpenChange={setOpenSheet} />
    </div>
  )
}

function RouteComponent() {
  const { isEditing, setIsEditing } = useDashboardContext()

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <div
          data-slot="dashboard-container"
          data-edit={String(isEditing)}
          className={cn(
            'relative size-full overflow-hidden transition-[padding] duration-300',
            'flex flex-col gap-3 md:gap-4',
            'data-[edit=true]:p-4',
            'data-[edit=true]:pt-3',
            'data-[edit=true]:md:p-6',
            'data-[edit=true]:md:pt-4',
          )}
        >
          <HeaderAction />

          <GridComponent />
        </div>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <ContextMenuItem onSelect={() => setIsEditing(true)}>
          <EditRounded className="size-4" />

          <span>{m.dashboard_context_menu_edit_widgets()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
}
