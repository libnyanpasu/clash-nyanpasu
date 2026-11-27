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
        <div className="bg-surface-container-high size-10 rounded-full" />

        <div className="bg-surface-container-high h-6 w-48 rounded-full" />
      </div>

      <div className="bg-surface-container-high rounded-extra-large h-52" />

      <div className="flex flex-col gap-1">
        <div className="bg-surface-container-high rounded-t-extra-large rounded-b-extra-small h-20" />

        <div className="bg-surface-container-high rounded-extra-small h-20" />

        <div className="bg-surface-container-high rounded-t-extra-small rounded-b-extra-large h-20" />
      </div>
    </div>
  )
}
