import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { useSetting } from '@nyanpasu/query'
import {
  WindowCloseBehavior,
  WindowCloseOverride,
  WindowCloseSettings,
  WindowCloseSettingsPatch_Serialize,
} from '@nyanpasu/rpc/types'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

const DEFAULT_SETTINGS = {
  global: 'destroy',
  main: 'inherit',
  editor: 'inherit',
  tray_menu: 'inherit',
} satisfies WindowCloseSettings

type WindowKind = Exclude<keyof WindowCloseSettings, 'global'>

const changeOf = (
  kind: WindowKind,
  behavior: WindowCloseOverride,
): WindowCloseSettingsPatch_Serialize => {
  switch (kind) {
    case 'main':
      return { main: behavior }
    case 'editor':
      return { editor: behavior }
    case 'tray_menu':
      return { tray_menu: behavior }
  }
}

function WindowCloseRow<V extends string>({
  slot,
  label,
  value,
  messages,
  onChange,
}: {
  slot: string
  label: string
  value: V
  messages: Record<V, string>
  onChange: (value: V) => void
}) {
  return (
    <DropdownMenu align="end">
      <DropdownMenuTrigger asChild>
        <SettingsCardContent data-slot={`${slot}-trigger`} asChild>
          <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>{label}</ItemLabelText>

                <ItemLabelDescription>{messages[value]}</ItemLabelDescription>
              </ItemLabel>

              <ArrowForwardIosRounded />
            </ItemContainer>
          </Button>
        </SettingsCardContent>
      </DropdownMenuTrigger>

      <DropdownMenuContent sideOffset={-16} alignOffset={16}>
        {Object.entries(messages).map(([key, message]) => (
          <DropdownMenuCheckboxItem
            checked={value === key}
            key={key}
            onSelect={() => onChange(key as V)}
          >
            {message as string}
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export default function WindowCloseBehaviorCard({
  showTrayMenu,
}: {
  showTrayMenu: boolean
}) {
  const { value, upsert } = useSetting('window_close')

  const settings = { ...DEFAULT_SETTINGS, ...value }

  const behaviorMessages = {
    destroy: m.settings_nyanpasu_window_close_destroy(),
    hide: m.settings_nyanpasu_window_close_hide(),
  } satisfies Record<WindowCloseBehavior, string>

  const overrideMessages = {
    inherit: m.settings_nyanpasu_window_close_inherit({
      behavior: behaviorMessages[settings.global],
    }),
    ...behaviorMessages,
  } satisfies Record<WindowCloseOverride, string>

  const kinds: { kind: WindowKind; label: string }[] = [
    { kind: 'main', label: m.settings_nyanpasu_window_close_main() },
    { kind: 'editor', label: m.settings_nyanpasu_window_close_editor() },
  ]

  if (showTrayMenu) {
    kinds.push({
      kind: 'tray_menu',
      label: m.settings_nyanpasu_window_close_tray_menu(),
    })
  }

  return (
    <SettingsCard data-slot="window-close-behavior-card">
      <WindowCloseRow
        slot="window-close-behavior-global"
        label={m.settings_nyanpasu_window_close_global()}
        value={settings.global}
        messages={behaviorMessages}
        onChange={(global) => upsert({ global })}
      />

      {kinds.map(({ kind, label }) => (
        <WindowCloseRow
          key={kind}
          slot={`window-close-behavior-${kind.replace('_', '-')}`}
          label={label}
          value={settings[kind]}
          messages={overrideMessages}
          onChange={(behavior) => upsert(changeOf(kind, behavior))}
        />
      ))}

      <p
        className="text-on-surface-variant px-5 pb-5 text-sm"
        data-slot="window-close-behavior-description"
      >
        {m.settings_nyanpasu_window_close_description()}
      </p>
    </SettingsCard>
  )
}
