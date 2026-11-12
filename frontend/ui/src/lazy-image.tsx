import { useState, type ComponentPropsWithoutRef } from 'react'
import { cn } from '@nyanpasu/utils'

export interface LazyImageProps extends ComponentPropsWithoutRef<'img'> {
  loadingClassName?: string
}

export function LazyImage({
  className,
  loadingClassName,
  onLoad,
  onError,
  src,
  ...props
}: LazyImageProps) {
  const [state, setState] = useState({ src, status: 'loading' })
  if (state.src !== src) {
    setState({ src, status: 'loading' })
  }
  const loading = state.src !== src || state.status === 'loading'
  const failed = state.src === src && state.status === 'error'

  return (
    <>
      <div
        className={cn(
          'inline-block animate-pulse bg-slate-200 ring-1 ring-slate-200 dark:bg-slate-700 dark:ring-slate-700',
          className,
          loadingClassName,
          loading || failed ? 'inline-block' : 'hidden',
          failed && 'animate-none',
        )}
        data-slot="lazy-image-placeholder"
        data-error={failed || undefined}
      />

      <img
        {...props}
        src={src}
        onLoad={(event) => {
          setState({ src, status: 'loaded' })
          onLoad?.(event)
        }}
        onError={(event) => {
          setState({ src, status: 'error' })
          onError?.(event)
        }}
        className={cn(className, loading || failed ? 'hidden' : 'inline-block')}
      />
    </>
  )
}
