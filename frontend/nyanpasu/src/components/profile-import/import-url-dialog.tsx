import AutoAwesomeRounded from '~icons/material-symbols/auto-awesome-rounded'
import InfoOutlineRounded from '~icons/material-symbols/info-outline-rounded'
import { filesize } from 'filesize'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { Input } from '@nyanpasu/ui/input'
import { Modal, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import { CircularProgress, LinearProgress } from '@nyanpasu/ui/progress'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import {
  useBlockTask,
  useBlockTaskContext,
} from '@/components/providers/block-task-provider'
import {
  calculateSubscriptionQuota,
  subscriptionExpiryTimestamp,
} from '@/components/widgets/dashboard-daily-utils'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { formatDate } from '@/utils/date'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { useProfileMutations, useProfileUrlProbe } from '@nyanpasu/query'
import type {
  FolderId,
  ProfileId,
  ProfileImportKind,
  ProfileUrlProbe,
  RemoteProfileOptionsPatch_Deserialize,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  definitionOf,
  IMPORT_KIND_LABELS,
  IMPORT_KINDS,
  IMPORT_SIGNAL_LABELS,
  transformOf,
} from './import-kind'

export type ImportUrlRequest = {
  url: string
  /** A name the user gave; `null` keeps the name the import derives. */
  name: string | null
  option: RemoteProfileOptionsPatch_Deserialize | null
  /** Folder a Config joins; transforms are never foldered. */
  folder?: FolderId | null
}

type ImportMode = 'remote' | 'local'

// Materialization rewrites the managed path to `{uid}.{ext}` on add.
const PENDING_FILE = 'pending.yaml'

const IMPORT_TASK = 'import-url'

/**
 * Downloads `request.url`, suggests what it is, and imports it the way the
 * user confirms: kept as a remote subscription or imported once as a local
 * profile. Closing resolves with the new profile, or `null` when cancelled.
 */
export default function ImportUrlDialog({
  open,
  attempt,
  request,
  onClose,
}: {
  open: boolean
  /** Identifies the request; a new one downloads anew and resets the choices. */
  attempt: number
  /** The last request, kept while the dialog closes. */
  request: ImportUrlRequest | null
  onClose: (uid: ProfileId | null) => void
}) {
  const probe = useProfileUrlProbe(request && { ...request, attempt })
  // Closing mid-import would report a cancel for a profile being created.
  const importing =
    useBlockTaskContext().getTask(IMPORT_TASK)?.status === 'pending'

  return (
    <Modal
      open={open}
      onOpenChange={(value) => !value && !importing && onClose(null)}
    >
      <ModalContent>
        <Card
          className="w-[min(28rem,calc(100vw-2rem))]"
          data-slot="import-url-dialog"
        >
          <CardHeader>
            <ModalTitle>{m.profile_import_url_title()}</ModalTitle>
          </CardHeader>

          {request && probe.data ? (
            <ImportUrlConfirm
              key={attempt}
              request={request}
              probe={probe.data}
              onClose={onClose}
            />
          ) : (
            <>
              <CardContent>
                {probe.isError && !probe.isFetching ? (
                  <p
                    data-slot="import-url-dialog-error"
                    className="text-error text-sm break-all"
                  >
                    {m.profile_import_url_probe_failed({
                      error: formatError(probe.error),
                    })}
                  </p>
                ) : (
                  <div
                    data-slot="import-url-dialog-probing"
                    className="flex items-center gap-3 py-2"
                  >
                    <CircularProgress className="size-6" indeterminate />

                    <span className="text-on-surface-variant text-sm">
                      {m.profile_import_url_probing()}
                    </span>
                  </div>
                )}
              </CardContent>

              <CardFooter className="gap-1">
                {probe.isError && !probe.isFetching && (
                  <Button onClick={() => probe.refetch()}>
                    {m.profile_import_url_retry()}
                  </Button>
                )}

                <Button onClick={() => onClose(null)}>
                  {m.common_cancel()}
                </Button>
              </CardFooter>
            </>
          )}
        </Card>
      </ModalContent>
    </Modal>
  )
}

function ImportUrlConfirm({
  request,
  probe,
  onClose,
}: {
  request: ImportUrlRequest
  probe: ProfileUrlProbe
  onClose: (uid: ProfileId | null) => void
}) {
  const { create } = useProfileMutations()

  const suggested =
    probe.classification.suggested &&
    probe.accepted.includes(probe.classification.suggested)
      ? probe.classification.suggested
      : null
  const [kind, setKind] = useState<ProfileImportKind | null>(suggested)
  const [mode, setMode] = useState<ImportMode>('remote')
  const [name, setName] = useState(request.name ?? probe.suggested_name)

  const reasons = probe.classification.evidence
    .filter((evidence) => evidence.kind === suggested)
    .map((evidence) => evidence.signal)
  // The server named it only when it sent a title; otherwise the name is the
  // one the import derives from the URL.
  const serverNamed = probe.classification.evidence.some(
    (evidence) => evidence.signal === 'profile_title',
  )

  const folderOf = (kind: ProfileImportKind) =>
    kind === 'config' ? (request.folder ?? null) : null

  const commit = async (kind: ProfileImportKind) => {
    if (mode === 'remote') {
      const outcome = await create.mutateAsync({
        type: 'url',
        data: {
          url: request.url,
          // The prefilled name stays unpinned, so it keeps following the
          // subscription; only a name the user gave is pinned.
          name:
            request.name == null && name.trim() === probe.suggested_name
              ? null
              : name.trim() || null,
          option: request.option,
          transform: transformOf(kind),
          folder: folderOf(kind),
        },
      })
      return outcome.value
    }

    const outcome = await create.mutateAsync({
      type: 'manual',
      data: {
        request: {
          metadata: { name: name.trim() || probe.suggested_name, desc: null },
          definition: definitionOf(kind, {
            type: 'local',
            binding: { type: 'managed', file: PENDING_FILE },
          }),
          folder: folderOf(kind),
        },
        fileData: probe.content,
      },
    })
    return outcome.value
  }

  const blockTask = useBlockTask(IMPORT_TASK, async () => {
    if (!kind) {
      return
    }

    try {
      onClose(await commit(kind))
    } catch (error) {
      message(m.profile_create_failed_message({ error: formatError(error) }), {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  const handleImport = useLockFn(blockTask.execute)

  return (
    <>
      <CardContent className="space-y-4 pt-2">
        <Input
          variant="outlined"
          label={m.profile_import_url_link_label()}
          value={request.url}
          title={request.url}
          readOnly
        />

        <ImportUrlSubscription probe={probe} />

        <div className="space-y-2" data-slot="import-url-dialog-kind">
          <p className="text-on-surface-variant px-1 text-sm">
            {m.profile_import_url_kind_label()}
          </p>

          <SegmentedButton
            size="sm"
            value={kind ?? ''}
            onValueChange={(value) =>
              value && setKind(value as ProfileImportKind)
            }
          >
            {IMPORT_KINDS.map((item) => (
              <SegmentedButtonItem
                key={item}
                value={item}
                disabled={!probe.accepted.includes(item)}
              >
                {IMPORT_KIND_LABELS[item]()}
              </SegmentedButtonItem>
            ))}
          </SegmentedButton>

          {suggested ? (
            <div
              data-slot="import-url-dialog-detection"
              className="flex flex-wrap items-center gap-1.5 px-1 text-xs"
            >
              <AutoAwesomeRounded
                className="text-primary size-4 shrink-0"
                aria-hidden
              />

              <span className="text-on-surface font-medium">
                {m.profile_import_url_detected({
                  kind: IMPORT_KIND_LABELS[suggested](),
                })}
              </span>

              {reasons.map((signal) => (
                <span
                  key={signal}
                  data-slot="import-url-dialog-reason"
                  className="bg-secondary-container text-on-secondary-container rounded-full px-2 py-0.5"
                >
                  {IMPORT_SIGNAL_LABELS[signal]()}
                </span>
              ))}
            </div>
          ) : (
            <p
              data-slot="import-url-dialog-detection"
              className={cn(
                'flex items-center gap-1.5 px-1 text-xs',
                probe.accepted.length === 0
                  ? 'text-error'
                  : 'text-on-surface-variant',
              )}
            >
              <InfoOutlineRounded className="size-4 shrink-0" aria-hidden />

              {probe.accepted.length === 0
                ? m.profile_import_url_unsupported()
                : m.profile_import_url_no_suggestion()}
            </p>
          )}
        </div>

        <div className="space-y-2" data-slot="import-url-dialog-mode">
          <p className="text-on-surface-variant px-1 text-sm">
            {m.profile_import_url_mode_label()}
          </p>

          <SegmentedButton
            size="sm"
            value={mode}
            onValueChange={(value) => value && setMode(value as ImportMode)}
          >
            <SegmentedButtonItem value="remote">
              {m.profile_source_remote()}
            </SegmentedButtonItem>

            <SegmentedButtonItem value="local">
              {m.profile_source_local()}
            </SegmentedButtonItem>
          </SegmentedButton>

          <p className="text-on-surface-variant px-1 text-xs">
            {mode === 'remote'
              ? m.profile_import_url_mode_remote_hint()
              : m.profile_import_url_mode_local_hint()}
          </p>
        </div>

        <div className="space-y-2">
          <Input
            variant="outlined"
            label={m.profile_form_name_label()}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />

          {request.name == null && (
            <p className="text-on-surface-variant px-1 text-xs">
              {serverNamed
                ? m.profile_import_url_name_from_server()
                : m.profile_import_url_name_from_link()}
            </p>
          )}
        </div>
      </CardContent>

      <CardFooter className="gap-1">
        <Button
          onClick={handleImport}
          loading={blockTask.isPending}
          disabled={!kind}
        >
          {m.profile_import_url_confirm()}
        </Button>

        <Button onClick={() => onClose(null)} disabled={blockTask.isPending}>
          {m.common_cancel()}
        </Button>
      </CardFooter>
    </>
  )
}

/** The subscription details the server sent, when it sent any. */
function ImportUrlSubscription({ probe }: { probe: ProfileUrlProbe }) {
  const quota = calculateSubscriptionQuota(probe.subscription)
  const expiry = subscriptionExpiryTimestamp(probe.subscription.expire)
  const interval = probe.suggested_update_interval_minutes

  if (!quota && !expiry && interval == null) {
    return null
  }

  return (
    <div
      data-slot="import-url-dialog-subscription"
      className="bg-surface-variant/30 dark:bg-surface-variant/10 space-y-1 rounded-xl p-3 text-sm"
    >
      {quota && (
        <>
          <p className="flex items-baseline justify-between gap-2">
            <span className="text-on-surface-variant">
              {m.profile_card_remaining()}
            </span>

            <span className="font-medium tabular-nums">
              {filesize(quota.remaining, { standard: 'iec' })}{' '}
              <span className="text-on-surface-variant font-normal">
                {m.profile_card_total({
                  total: filesize(quota.total, { standard: 'iec' }),
                })}
              </span>
            </span>
          </p>

          <LinearProgress
            className="h-1.5"
            value={quota.remainingPercent}
            role="meter"
            aria-label={m.profile_card_remaining()}
          />
        </>
      )}

      {expiry && (
        <p className="text-on-surface-variant text-xs">
          {m.profile_subscription_expires_in({
            expires: formatDate(expiry, 'yyyy-MM-dd'),
          })}
        </p>
      )}

      {interval != null && (
        <p className="text-on-surface-variant text-xs">
          {m.profile_import_url_update_interval({ minutes: interval })}
        </p>
      )}
    </div>
  )
}
