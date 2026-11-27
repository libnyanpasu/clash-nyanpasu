import {
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
} from '@nyanpasu/ui/item-group'
import { RequireCapability } from '@/components/auth/capabilities'
import { ReadOnlyScope } from '@/components/auth/read-only-scope'
import { RestrictedContent } from '@/components/auth/restricted-content'
import { PageTitle } from '@/components/page-title'
import { m } from '@/paraglide/messages'
import { Platform, PlatformWindows } from '@nyanpasu/platform/react'
import { createFileRoute } from '@tanstack/react-router'
import AutoLaunchSwitch from './_modules/auto-launch-switch'
import DnsCacheButton from './_modules/dns-cache-button'
import SilentLaunchSwitch from './_modules/slient-launch-switch'
import SystemServiceCtrl from './_modules/system-service-ctrl'
import SystemServiceSwitch from './_modules/system-service-switch'
import UwpToolsButton from './_modules/uwp-tools-button'

export const Route = createFileRoute('/(main)/main/settings/system/service/')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_service() } },
})

const SystemService = () => {
  return (
    <ItemSection data-slot="system-service-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_system_proxy_system_service_ctrl_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <SystemServiceSwitch />

        <SystemServiceCtrl />
      </ItemGroup>
    </ItemSection>
  )
}

const SystemLaunch = () => {
  return (
    <ItemSection data-slot="system-launch-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_system_proxy_launch_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <AutoLaunchSwitch />

        <SilentLaunchSwitch />
      </ItemGroup>
    </ItemSection>
  )
}

const SystemTools = () => {
  return (
    <ItemSection data-slot="system-tools-container">
      <ItemSectionHeader>
        <ItemSectionTitle>
          {m.settings_system_proxy_system_tools_label()}
        </ItemSectionTitle>
      </ItemSectionHeader>

      <ItemGroup>
        <DnsCacheButton />

        <PlatformWindows>
          <UwpToolsButton />
        </PlatformWindows>
      </ItemGroup>
    </ItemSection>
  )
}

function RouteComponent() {
  return (
    <>
      <PageTitle />

      <RequireCapability
        permission="system:read"
        fallback={<RestrictedContent />}
      >
        <ReadOnlyScope permissions={['system:write']}>
          <div className="space-y-4 px-4 pb-4">
            <SystemService />

            <SystemLaunch />

            <Platform when={['windows', 'macos']}>
              <SystemTools />
            </Platform>
          </div>
        </ReadOnlyScope>
      </RequireCapability>
    </>
  )
}
