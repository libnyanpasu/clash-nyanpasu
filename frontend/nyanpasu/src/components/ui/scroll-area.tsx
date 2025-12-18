import * as React from 'react'
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
  return (
    <Root
      data-slot="scroll-area"
      className={cn('relative', className)}
      {...props}
    >
      <Viewport>{children}</Viewport>
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
        'flex touch-none p-px transition-colors select-none',
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
