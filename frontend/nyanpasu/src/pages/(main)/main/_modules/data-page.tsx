import { cva } from 'class-variance-authority'
import {
  createContext,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentProps,
  type CSSProperties,
  type ReactNode,
} from 'react'
import { cn } from '@nyanpasu/utils'

export const dataPageLayoutVariants = cva(
  'relative flex min-h-0 w-full flex-1 overflow-hidden',
)

const DataPageBottomInsetContext = createContext(0)

/** Space below the last row so it can scroll clear of the floating controls. */
export function useDataPageBottomInset() {
  return useContext(DataPageBottomInsetContext)
}

export function DataPageToolbar({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'pointer-events-auto flex min-w-0 flex-wrap items-center gap-2',
        className,
      )}
      data-slot="data-page-toolbar"
      {...props}
    />
  )
}

export function DataPage({
  title,
  toolbar,
  children,
}: {
  title: string
  toolbar: ReactNode
  children: ReactNode
}) {
  const toolbarRef = useRef<HTMLDivElement>(null)
  const [toolbarHeight, setToolbarHeight] = useState(0)
  const bottomInset = toolbarHeight + 40

  useLayoutEffect(() => {
    const toolbar = toolbarRef.current
    if (!toolbar) return

    const measure = () => setToolbarHeight(toolbar.offsetHeight)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(toolbar)
    return () => observer.disconnect()
  }, [])

  return (
    <DataPageBottomInsetContext.Provider value={bottomInset}>
      <section
        className="bg-mixed-background @container relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
        data-slot="data-page"
        aria-label={title}
        style={
          {
            // Dock bottom (8px) plus an 8px gap; row padding is independent.
            '--data-page-scrollbar-inset': `${toolbarHeight + 16}px`,
          } as CSSProperties
        }
      >
        {children}

        <div
          ref={toolbarRef}
          className="pointer-events-none absolute inset-x-2 bottom-2 z-30 md:inset-x-3"
          data-slot="data-page-dock"
        >
          {toolbar}
        </div>
      </section>
    </DataPageBottomInsetContext.Provider>
  )
}

export function DataPageSurface({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex min-h-0 min-w-0 flex-1 flex-col',
        // Radix pins tracks with inline styles; keep them clear of the dock.
        '[&_[data-slot=scroll-area-scrollbar][data-orientation=horizontal]]:bottom-(--data-page-scrollbar-inset)!',
        '[&_[data-slot=scroll-area-scrollbar][data-orientation=horizontal]]:ml-3',
        '[&_[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:h-auto',
        '[&_[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:bottom-(--data-page-scrollbar-inset)!',
        '[&:has([data-slot=scroll-area-scrollbar][data-orientation=horizontal])_[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:bottom-[calc(var(--data-page-scrollbar-inset)+1.5rem)]!',
        className,
      )}
      data-slot="data-page-surface"
      {...props}
    />
  )
}
