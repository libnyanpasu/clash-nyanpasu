import { m } from '@/paraglide/messages'

/** Holds the layout of the page while the profile list loads. */
export default function ProfileDetailSkeleton() {
  return (
    <div
      role="status"
      aria-label={m.profile_detail_loading()}
      data-slot="profile-detail-skeleton"
      className="flex flex-col gap-4 motion-safe:animate-pulse"
    >
      <div className="flex h-16 items-center gap-4">
        <div className="bg-surface-variant size-10 rounded-full" />

        <div className="bg-surface-variant h-6 w-48 rounded-full" />
      </div>

      <div className="bg-surface-variant/60 h-52 rounded-3xl" />

      <div className="flex flex-col gap-1">
        <div className="bg-surface-variant/60 h-20 rounded-t-3xl rounded-b-sm" />

        <div className="bg-surface-variant/60 h-20 rounded-sm" />

        <div className="bg-surface-variant/60 h-20 rounded-t-sm rounded-b-3xl" />
      </div>
    </div>
  )
}
