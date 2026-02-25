import { CircularProgress } from '@/components/ui/progress'

export default function LoadingSkeleton() {
  return (
    <div className="grid flex-1 place-items-center">
      <CircularProgress className="size-12" indeterminate />
    </div>
  )
}
