import UploadFileRounded from '~icons/material-symbols/upload-file-rounded'
import { filesize } from 'filesize'
import { AnimatePresence } from 'motion/react'
import { ReactNode } from 'react'
import { Controller, useFormContext, useWatch } from 'react-hook-form'
import { Button } from '@/components/ui/button'
import {
  FileDropZone,
  FileDropZoneFileSelected,
  FileDropZoneLoading,
  FileDropZonePlaceholder,
} from '@/components/ui/file-drop-zone'
import { Input, NumericInput } from '@/components/ui/input'
import { CircularProgress } from '@/components/ui/progress'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import { SwitchItem } from '@/components/ui/switch'
import { m } from '@/paraglide/messages'
import { isConfigItem, useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { open } from '@tauri-apps/plugin-dialog'
import AnimatedErrorItem from '../../_modules/error-item'
import { ACCEPT_EXTENSIONS, type FormValues } from './create-profile-schema'

const FieldError = ({ message }: { message?: string }) => (
  <AnimatePresence>
    {message && (
      <AnimatedErrorItem className="text-error text-sm">
        {message}
      </AnimatedErrorItem>
    )}
  </AnimatePresence>
)

const FieldHint = ({ children }: { children: ReactNode }) => (
  <p className="text-on-surface-variant px-1 text-xs">{children}</p>
)

export const MetadataFields = () => {
  const { control } = useFormContext<FormValues>()
  const source = useWatch({ control, name: 'source' })
  const kind = useWatch({ control, name: 'kind' })

  return (
    <>
      <Controller
        control={control}
        name="name"
        render={({ field }) => (
          <div className="space-y-2">
            <Input
              variant="outlined"
              label={m.profile_form_name_label()}
              {...field}
            />

            {source === 'remote' && kind !== 'composition' && (
              <FieldHint>{m.profile_remote_name_hint()}</FieldHint>
            )}
          </div>
        )}
      />

      <Controller
        control={control}
        name="desc"
        render={({ field }) => (
          <Input
            variant="outlined"
            label={m.profile_form_desc_label()}
            {...field}
          />
        )}
      />
    </>
  )
}

export const RemoteFields = () => {
  const { control } = useFormContext<FormValues>()

  return (
    <>
      <Controller
        control={control}
        name="url"
        render={({ field, fieldState }) => (
          <div className="space-y-2">
            <Input
              variant="outlined"
              label={m.profile_form_url_label()}
              {...field}
            />

            <FieldError message={fieldState.error?.message} />
          </div>
        )}
      />

      <Controller
        control={control}
        name="option.user_agent"
        render={({ field }) => (
          <Input
            variant="outlined"
            label={m.profile_form_option_user_agent_label()}
            {...field}
          />
        )}
      />

      <Controller
        control={control}
        name="option.update_interval"
        render={({ field, fieldState }) => (
          <div className="space-y-2">
            <NumericInput
              variant="outlined"
              label={m.profile_form_option_update_interval_label()}
              min={1}
              step={1}
              {...field}
            />

            <FieldHint>
              {m.profile_form_option_update_interval_placeholder()}
            </FieldHint>

            <FieldError message={fieldState.error?.message} />
          </div>
        )}
      />

      <Controller
        control={control}
        name="option.with_proxy"
        render={({ field }) => (
          <SwitchItem
            checked={field.value}
            onCheckedChange={(checked) => field.onChange(checked)}
          >
            <span>{m.profile_with_proxy_label()}</span>
          </SwitchItem>
        )}
      />

      <Controller
        control={control}
        name="option.self_proxy"
        render={({ field }) => (
          <SwitchItem
            checked={field.value}
            onCheckedChange={(checked) => field.onChange(checked)}
          >
            <span>{m.profile_self_proxy_label()}</span>
          </SwitchItem>
        )}
      />
    </>
  )
}

export const LocalFileField = ({ disabled }: { disabled: boolean }) => {
  const { control, setValue } = useFormContext<FormValues>()
  const kind = useWatch({ control, name: 'kind' })
  const fileContent = useWatch({ control, name: 'fileContent' })
  const accept = ACCEPT_EXTENSIONS[kind]

  return (
    <div className="space-y-2">
      <Controller
        control={control}
        name="fileName"
        render={({ field }) => (
          <FileDropZone
            accept={accept}
            value={field.value}
            onChange={(path) => field.onChange(path)}
            onFileRead={(content) => setValue('fileContent', content)}
            disabled={disabled}
          >
            <FileDropZonePlaceholder className="flex flex-col items-center justify-center gap-2">
              <UploadFileRounded className="text-on-surface-variant size-8" />

              <span className="text-on-surface-variant text-sm">
                {m.profile_import_local_file_placeholder()}
              </span>

              <span className="text-on-surface-variant text-xs">
                {m.profile_import_local_file_type_label({
                  types: accept.join(', '),
                })}
              </span>
            </FileDropZonePlaceholder>

            <FileDropZoneLoading>
              <CircularProgress className="size-8" indeterminate />
            </FileDropZoneLoading>

            <FileDropZoneFileSelected className="flex flex-col items-center justify-center gap-2">
              <UploadFileRounded className="text-primary size-8" />

              <div className="text-on-surface max-w-full truncate text-sm font-medium">
                {m.profile_import_local_file_size_label({
                  size: filesize(
                    fileContent ? new Blob([fileContent]).size : 0,
                    { standard: 'iec' },
                  ),
                })}
              </div>
            </FileDropZoneFileSelected>
          </FileDropZone>
        )}
      />

      <FieldHint>{m.profile_local_template_hint()}</FieldHint>
    </div>
  )
}

export const ExternalFileField = ({ disabled }: { disabled: boolean }) => {
  const { control } = useFormContext<FormValues>()
  const kind = useWatch({ control, name: 'kind' })
  const mode = useWatch({ control, name: 'externalMode' })

  return (
    <>
      <Controller
        control={control}
        name="externalPath"
        render={({ field, fieldState }) => (
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <code
                className={cn(
                  'bg-surface-variant/30 dark:bg-surface-variant/10',
                  'min-w-0 flex-1 truncate rounded-xl px-3 py-2.5 text-xs',
                  !field.value && 'text-on-surface-variant',
                )}
                title={field.value || undefined}
              >
                {field.value || '-'}
              </code>

              <Button
                variant="stroked"
                className="shrink-0"
                disabled={disabled}
                onClick={async () => {
                  const selected = await open({
                    directory: false,
                    multiple: false,
                    filters: [
                      {
                        name: 'Profile',
                        extensions: ACCEPT_EXTENSIONS[kind].map((ext) =>
                          ext.slice(1),
                        ),
                      },
                    ],
                  })

                  if (typeof selected === 'string') {
                    field.onChange(selected)
                  }
                }}
              >
                {m.profile_external_pick()}
              </Button>
            </div>

            <FieldError message={fieldState.error?.message} />
          </div>
        )}
      />

      <Controller
        control={control}
        name="externalMode"
        render={({ field }) => (
          <div className="space-y-2">
            <SegmentedButton
              size="sm"
              value={field.value}
              onValueChange={(value) => value && field.onChange(value)}
            >
              <SegmentedButtonItem value="mirror">
                {m.profile_external_mode_mirror()}
              </SegmentedButtonItem>

              <SegmentedButtonItem value="symlink">
                {m.profile_external_mode_symlink()}
              </SegmentedButtonItem>
            </SegmentedButton>

            <FieldHint>
              {mode === 'mirror'
                ? m.profile_external_mode_mirror_hint()
                : m.profile_external_mode_symlink_hint()}
            </FieldHint>
          </div>
        )}
      />
    </>
  )
}

export const CompositionMembersField = () => {
  const { control } = useFormContext<FormValues>()
  const { query } = useProfile()

  // Composition members must be direct File configs (not Composition/Transform).
  const candidates = (query.data?.items ?? []).filter(
    (item) => isConfigItem(item) && item.config.type === 'file',
  )

  return (
    <Controller
      control={control}
      name="members"
      render={({ field, fieldState }) => {
        const selected = new Set(field.value)

        const toggle = (uid: string) =>
          field.onChange(
            selected.has(uid)
              ? field.value.filter((member) => member !== uid)
              : [...field.value, uid],
          )

        return (
          <div className="space-y-2">
            <FieldHint>{m.profile_composition_min_members_hint()}</FieldHint>

            <div className="flex flex-col gap-2">
              {candidates.map((item) => (
                <Button
                  key={item.uid}
                  variant="raised"
                  aria-pressed={selected.has(item.uid)}
                  className={cn(
                    'justify-start',
                    selected.has(item.uid) &&
                      'bg-primary-container dark:bg-surface-variant',
                  )}
                  onClick={() => toggle(item.uid)}
                >
                  {item.name}
                </Button>
              ))}
            </div>

            <FieldError message={fieldState.error?.message} />
          </div>
        )
      }}
    />
  )
}
