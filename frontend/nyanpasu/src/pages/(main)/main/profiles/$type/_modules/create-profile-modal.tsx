import { useEffect } from 'react'
import { FormProvider, useForm, useWatch } from 'react-hook-form'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { Modal, ModalContent, ModalTitle } from '@/components/ui/modal'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import {
  useProfile,
  type NewProfileRequest_Deserialize,
  type ProfileSource_Deserialize,
} from '@nyanpasu/interface'
import {
  CompositionMembersField,
  ExternalFileField,
  LocalFileField,
  MetadataFields,
  RemoteFields,
} from './create-profile-fields'
import {
  CONFIG_KINDS,
  definitionOf,
  fallbackName,
  formSchema,
  getDefaultValues,
  KIND_LABELS,
  templateOf,
  TRANSFORM_KINDS,
  transformKindOf,
  type CreateKind,
  type CreateSource,
  type FormValues,
} from './create-profile-schema'

const SOURCE_LABELS = {
  remote: () => m.profile_source_remote(),
  local: () => m.profile_source_local(),
  external: () => m.profile_source_external(),
} satisfies Record<CreateSource, () => string>

// Materialization rewrites the managed path to `{uid}.{ext}` on add.
const PENDING_FILE = 'pending.yaml'

export default function CreateProfileModal({
  open,
  onOpenChange,
  kind,
  source,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Initial kind; also decides whether config or transform kinds are offered. */
  kind: CreateKind
  source: CreateSource
}) {
  const { create, patchMetadata } = useProfile()

  const isConfig = (CONFIG_KINDS as readonly CreateKind[]).includes(kind)

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: getDefaultValues(kind, source),
  })

  useEffect(() => {
    if (open) {
      form.reset(getDefaultValues(kind, source))
    }
  }, [open, kind, source, form])

  const currentKind = useWatch({ control: form.control, name: 'kind' })
  const currentSource = useWatch({ control: form.control, name: 'source' })

  const createRemote = async (values: FormValues) => {
    const outcome = await create.mutateAsync({
      type: 'url',
      data: {
        url: values.url,
        // Empty keeps the name derived server-side and synced from the
        // subscription.
        name: values.name.trim() || null,
        option: {
          user_agent: values.option.user_agent || null,
          with_proxy: values.option.with_proxy,
          self_proxy: values.option.self_proxy,
          update_interval_minutes: values.option.update_interval,
        },
        transform: transformKindOf(values.kind),
      },
    })

    // The profile exists once import returns, so a failed desc patch must not
    // be reported as a create failure (retrying would import a duplicate).
    if (values.desc) {
      try {
        await patchMetadata.mutateAsync({
          uid: outcome.value,
          patch: { name: null, desc: values.desc },
        })
      } catch (error) {
        message(
          m.profile_import_rename_failed_message({
            error: formatError(error),
          }),
          { title: 'Warning', kind: 'warning' },
        )
      }
    }
  }

  const createManual = async (values: FormValues) => {
    const metadata = { name: fallbackName(values), desc: values.desc || null }

    let request: NewProfileRequest_Deserialize
    let fileData: string | null = null

    if (values.kind === 'composition') {
      request = {
        metadata,
        definition: {
          type: 'config',
          config: {
            type: 'composition',
            base: null,
            extend_proxies_from: values.members,
            transforms: [],
          },
        },
      }
    } else {
      const profileSource: ProfileSource_Deserialize =
        values.source === 'external'
          ? {
              type: 'local',
              binding: {
                type: 'external',
                file: PENDING_FILE,
                target: values.externalPath,
                mode: values.externalMode,
              },
            }
          : {
              type: 'local',
              binding: { type: 'managed', file: PENDING_FILE },
            }

      request = {
        metadata,
        definition: definitionOf(values.kind, profileSource),
      }

      if (values.source === 'local') {
        fileData = values.fileContent || templateOf(values.kind)
      }
    }

    await create.mutateAsync({ type: 'manual', data: { request, fileData } })
  }

  const blockTask = useBlockTask(
    'create-profile',
    form.handleSubmit(async (values) => {
      try {
        if (values.kind !== 'composition' && values.source === 'remote') {
          await createRemote(values)
        } else {
          await createManual(values)
        }

        onOpenChange(false)
      } catch (error) {
        message(
          m.profile_create_failed_message({ error: formatError(error) }),
          { title: 'Error', kind: 'error' },
        )
      }
    }),
  )

  const handleSubmit = useLockFn(blockTask.execute)

  const handleOpenChange = (value: boolean) => {
    if (blockTask.isPending) {
      return
    }

    onOpenChange(value)
  }

  const kinds = isConfig ? CONFIG_KINDS : TRANSFORM_KINDS

  return (
    <Modal open={open} onOpenChange={handleOpenChange}>
      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>
              {isConfig
                ? m.profile_create_config_title()
                : m.profile_create_transform_title()}
            </ModalTitle>
          </CardHeader>

          <CardContent asChild>
            <ScrollArea className="max-h-[80dvh]">
              <FormProvider {...form}>
                <div className="space-y-4 pt-2">
                  <SegmentedButton
                    size="sm"
                    value={currentKind}
                    onValueChange={(value) => {
                      if (value) {
                        form.setValue('kind', value as CreateKind)
                        // accepted file types follow the kind
                        form.setValue('fileName', null)
                        form.setValue('fileContent', null)
                        form.setValue('externalPath', '')
                      }
                    }}
                  >
                    {kinds.map((item) => (
                      <SegmentedButtonItem key={item} value={item}>
                        {KIND_LABELS[item]()}
                      </SegmentedButtonItem>
                    ))}
                  </SegmentedButton>

                  {currentKind !== 'composition' && (
                    <SegmentedButton
                      size="sm"
                      value={currentSource}
                      onValueChange={(value) => {
                        if (value) {
                          form.setValue('source', value as CreateSource)
                          form.clearErrors()
                        }
                      }}
                    >
                      {(['remote', 'local', 'external'] as const).map(
                        (item) => (
                          <SegmentedButtonItem key={item} value={item}>
                            {SOURCE_LABELS[item]()}
                          </SegmentedButtonItem>
                        ),
                      )}
                    </SegmentedButton>
                  )}

                  <MetadataFields />

                  {currentKind === 'composition' ? (
                    <CompositionMembersField />
                  ) : currentSource === 'remote' ? (
                    <RemoteFields />
                  ) : currentSource === 'local' ? (
                    <LocalFileField disabled={blockTask.isPending} />
                  ) : (
                    <ExternalFileField disabled={blockTask.isPending} />
                  )}
                </div>
              </FormProvider>
            </ScrollArea>
          </CardContent>

          <CardFooter className="gap-1">
            <Button onClick={handleSubmit} loading={blockTask.isPending}>
              {m.common_submit()}
            </Button>

            <Button onClick={() => handleOpenChange(false)}>
              {m.common_cancel()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}
