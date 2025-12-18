import * as React from 'react'
import { useRef, useState } from 'react'
import { cn } from '@nyanpasu/ui'
import * as ScrollAreaPrimitive from '@radix-ui/react-scroll-area'

export function Viewport({
  className,
  children,
  ...props
}: React.ComponentProps<typeof ScrollAreaPrimitive.Viewport>) {
  return (
    <ScrollAreaPrimitive.Viewport
      data-slot="scroll-area-viewport"
      className={cn(
        'size-full rounded-[inherit] transition-[color,box-shadow] outline-none',
        className,
      )}
      {...props}
    >
      {children}
    </ScrollAreaPrimitive.Viewport>
  )
}

export const Corner = ScrollAreaPrimitive.Corner

export const Root = ScrollAreaPrimitive.Root

export function ScrollArea({
  className,
  children,
  ...props
}: React.ComponentProps<typeof ScrollAreaPrimitive.Root>) {
  const [isTop, setIsTop] = useState(true)

  const [scrollDirection, setScrollDirection] = useState<
    'up' | 'down' | 'left' | 'right' | 'none'
  >('none')

  const lastScrollTop = useRef(0)
  const lastScrollLeft = useRef(0)

  const handleScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const target = e.currentTarget as HTMLElement
    const { scrollTop, scrollLeft } = target

    setIsTop(scrollTop === 0)

    const deltaY = scrollTop - lastScrollTop.current
    const deltaX = scrollLeft - lastScrollLeft.current

    // Determine primary scroll direction
    if (Math.abs(deltaY) > Math.abs(deltaX)) {
      if (deltaY > 0) {
        setScrollDirection('down')
      } else if (deltaY < 0) {
        setScrollDirection('up')
      }
    } else if (Math.abs(deltaX) > Math.abs(deltaY)) {
      if (deltaX > 0) {
        setScrollDirection('right')
      } else if (deltaX < 0) {
        setScrollDirection('left')
      }
    }

    lastScrollTop.current = scrollTop
    lastScrollLeft.current = scrollLeft
  }

  return (
    <Root
      data-slot="scroll-area"
      type="scroll"
      scrollHideDelay={600}
      className={cn('relative', className)}
      data-top={String(isTop)}
      data-scroll-direction={scrollDirection}
      {...props}
    >
      <Viewport onScroll={handleScroll}>{children}</Viewport>
      <ScrollBar />
      <Corner />
    </Root>
  )
}

export function ScrollBar({
  className,
  orientation = 'vertical',
  ...props
}: React.ComponentProps<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>) {
  return (
    <ScrollAreaPrimitive.ScrollAreaScrollbar
      data-slot="scroll-area-scrollbar"
      orientation={orientation}
      className={cn(
        'z-50 flex touch-none p-px select-none',
        'transition-opacity duration-300 ease-out',
        'data-[state=hidden]:opacity-0 data-[state=visible]:opacity-100',
        orientation === 'vertical' &&
          'h-full w-2.5 border-l border-l-transparent py-1',
        orientation === 'horizontal' &&
          'h-2.5 flex-col border-t border-t-transparent px-1',
        className,
      )}
      {...props}
    >
      <ScrollAreaPrimitive.ScrollAreaThumb
        data-slot="scroll-area-thumb"
        className="bg-surface-variant relative flex-1 rounded-full"
      />
    </ScrollAreaPrimitive.ScrollAreaScrollbar>
  )
}

export function AppContentScrollArea({
  className,
  children,
  ...props
}: React.ComponentProps<typeof ScrollAreaPrimitive.Root>) {
  return (
    <ScrollArea
      className={cn(
        'flex flex-1 flex-col',
        'max-h-[calc(100vh-40px-64px)]',
        'min-h-[calc(100vh-40px-64px)]',
        'sm:max-h-[calc(100vh-40px-48px)]',
        'sm:min-h-[calc(100vh-40px-48px)]',
        className,
      )}
      data-slot="app-content-scroll-area"
      {...props}
    >
      {children}
    </ScrollArea>
  )
}
