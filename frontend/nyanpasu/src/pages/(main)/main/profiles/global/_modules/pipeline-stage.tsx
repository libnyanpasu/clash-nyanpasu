import type { ComponentProps, ReactNode } from 'react'
import { cn } from '@nyanpasu/utils'

/** One numbered stage of the pipeline: a small header above its content. */
export default function PipelineStage({
  index,
  name,
  description,
  className,
  children,
  ...props
}: Omit<ComponentProps<'li'>, 'children'> & {
  index: number
  name: string
  description: string
  children: ReactNode
}) {
  return (
    <li
      data-slot="profile-global-pipeline-stage"
      className={cn('flex min-w-0 flex-col gap-2', className)}
      {...props}
    >
      <div className="flex items-center gap-3 px-1">
        <span
          data-slot="profile-global-pipeline-stage-index"
          className="bg-tertiary-container text-on-tertiary-container grid size-6 shrink-0 place-items-center rounded-md text-xs font-medium tabular-nums"
        >
          {index}
        </span>

        <div className="min-w-0">
          <h3
            data-slot="profile-global-pipeline-stage-name"
            className="text-xs font-medium tracking-wider uppercase"
          >
            {name}
          </h3>

          <p className="text-on-surface-variant text-xs">{description}</p>
        </div>
      </div>

      {children}
    </li>
  )
}
