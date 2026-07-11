import { PropsWithChildren, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import {
  isConfigItem,
  useProfile,
  type NewProfileRequest_Deserialize,
  type ProfileId,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'

/**
 * Minimal Composition creation (design §11.3): pick >=2 File Config profiles as
 * proxy contributors and create a Composition. A full management surface
 * (editing members / base selection) is intentionally out of scope.
 */
export default function CreateCompositionButton({
  children,
}: PropsWithChildren) {
  const { query, create } = useProfile()

  const [open, setOpen] = useState(false)
  const [name, setName] = useState<string>(() =>
    m.profile_composition_default_name(),
  )
  const [selected, setSelected] = useState<Set<ProfileId>>(new Set())

  // Composition members must be direct File configs (not Composition/Transform).
  const candidates = (query.data?.items ?? []).filter(
    (item) => isConfigItem(item) && item.config.type === 'file',
  )

  const reset = () => {
    setSelected(new Set())
    setName(m.profile_composition_default_name())
  }

  const toggle = (uid: ProfileId) =>
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(uid)) {
        next.delete(uid)
      } else {
        next.add(uid)
      }
      return next
    })

  const submit = useLockFn(async () => {
    const members = [...selected]
    if (members.length < 2) {
      return
    }

    const request: NewProfileRequest_Deserialize = {
      metadata: { name, desc: null },
      definition: {
        type: 'config',
        config: {
          type: 'composition',
          base: null,
          extend_proxies_from: members,
          transforms: [],
        },
      },
    }

    try {
      await create.mutateAsync({
        type: 'manual',
        data: { request, fileData: null },
      })
      setOpen(false)
      reset()
    } catch (error) {
      message(m.profile_create_failed_message({ error: formatError(error) }), {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  return (
    <Modal
      open={open}
      onOpenChange={(value) => {
        setOpen(value)
        if (value) {
          reset()
        }
      }}
    >
      <ModalTrigger asChild>{children}</ModalTrigger>

      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>{m.profile_create_composition_title()}</ModalTitle>
          </CardHeader>

          <CardContent asChild>
            <ScrollArea className="max-h-[80dvh]">
              <div className="space-y-4 pt-2">
                <Input
                  variant="outlined"
                  label={m.profile_form_name_label()}
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                />

                <div className="text-sm opacity-60">
                  {m.profile_composition_min_members_hint()}
                </div>

                <div className="flex flex-col gap-2">
                  {candidates.map((item) => (
                    <Button
                      key={item.uid}
                      variant="raised"
                      aria-pressed={selected.has(item.uid)}
                      data-selected={String(selected.has(item.uid))}
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
              </div>
            </ScrollArea>
          </CardContent>

          <CardFooter className="gap-1">
            <Button
              onClick={submit}
              loading={create.isPending}
              disabled={selected.size < 2}
            >
              {m.common_submit()}
            </Button>

            <Button onClick={() => setOpen(false)}>{m.common_cancel()}</Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  )
}
