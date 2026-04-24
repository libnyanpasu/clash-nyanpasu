import { useState, type ComponentPropsWithoutRef } from 'react'
import { cn } from './classnames'

export interface LazyImageProps extends ComponentPropsWithoutRef<'img'> {
  loadingClassName?: string
}

export function LazyImage({
  className,
  loadingClassName,
  onLoad,
  ...props
}: LazyImageProps) {
  const [loading, setLoading] = useState(true)

  return (
    <>
      <div
        className={cn(
          'inline-block animate-pulse bg-slate-200 ring-1 ring-slate-200 dark:bg-slate-700 dark:ring-slate-700',
          className,
          loadingClassName,
          loading ? 'inline-block' : 'hidden',
        )}
      />

      <img
        {...props}
        onLoad={(event) => {
          setLoading(false)
          onLoad?.(event)
        }}
        className={cn(className, loading ? 'hidden' : 'inline-block')}
      />
    </>
  )
}
