import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import { Button } from '@nyanpasu/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { usePlatform } from '@nyanpasu/platform/react'
import { invokeMutation, useFullSetting, useQueryApi } from '@nyanpasu/query'
import { type ProfileFileOpener } from '@nyanpasu/rpc/types'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../../../_modules/settings-card'

export default function ProfileFileOpenerSelector() {
  const { clientOs } = usePlatform()
  const { value } = useFullSetting('profile_file_opener')
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const command = api.mutations.setProfileFileOpener()
  const update = useMutation({
    mutationKey: command.mutationKey,
    mutationFn: (opener: ProfileFileOpener) =>
      invokeMutation(command, { opener }),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: api.queries.getAppConfig().queryKey,
      }),
  })

  const options = [
    {
      type: 'system_default',
      label: m.settings_user_interface_file_opener_default(),
    },
    { type: 'vs_code', label: 'VS Code' },
    { type: 'zed', label: 'Zed' },
    { type: 'notepad_plus_plus', label: 'Notepad++' },
    { type: 'custom', label: m.settings_user_interface_file_opener_custom() },
  ] as const

  const selected = options.find((option) => option.type === value?.type)
  const description = value?.type === 'custom' ? value.path : selected?.label

  const handleSelect = useLockFn(async (type: ProfileFileOpener['type']) => {
    try {
      if (type === 'custom') {
        const path = await api.rpc.openNativeFileDialog(
          m.settings_user_interface_file_opener_custom(),
          clientOs === 'windows'
            ? [
                {
                  name: m.settings_user_interface_file_opener_custom(),
                  extensions: ['exe'],
                },
              ]
            : [],
        )
        if (path) await update.mutateAsync({ type, path })
        return
      }
      await update.mutateAsync({ type })
    } catch (error) {
      await message(formatError(error), {
        title: m.settings_user_interface_file_opener(),
        kind: 'error',
        error,
      })
    }
  })

  return (
    <SettingsCard data-slot="profile-file-opener-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent asChild>
            <Button
              className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
              disabled={!value || update.isPending}
            >
              <ItemContainer>
                <ItemLabel className="min-w-0">
                  <ItemLabelText>
                    {m.settings_user_interface_file_opener()}
                  </ItemLabelText>
                  <ItemLabelDescription className="truncate">
                    {description}
                  </ItemLabelDescription>
                </ItemLabel>
                <ArrowForwardIosRounded aria-hidden />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>
        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {options
            .filter(
              (option) =>
                option.type !== 'notepad_plus_plus' || clientOs === 'windows',
            )
            .map(({ type, label }) => (
              <DropdownMenuCheckboxItem
                key={type}
                checked={value?.type === type}
                onSelect={() => handleSelect(type)}
              >
                {label}
              </DropdownMenuCheckboxItem>
            ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  )
}
