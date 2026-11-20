import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { cn } from '@nyanpasu/utils'
import { useDnsDraft } from './dns-draft'
import DnsYamlModal from './dns-yaml-modal'

/**
 * Saves or discards the draft as a whole. A failed save keeps the draft, so
 * the core's complaint can be fixed in place.
 */
export default function DnsSaveBar() {
  const { dirty, saving, save, discard } = useDnsDraft()

  const handleSave = useLockFn(async () => {
    try {
      await save()
    } catch (error) {
      message(`${m.settings_clash_dns_save_failed()}\n${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  return (
    <div
      className={cn(
        'sticky bottom-0 z-10 flex flex-wrap items-center gap-2 py-3',
        'bg-surface/90 backdrop-blur',
      )}
      data-slot="dns-save-bar"
    >
      <DnsYamlModal />

      <div className="flex-1" />

      <Button variant="stroked" disabled={!dirty || saving} onClick={discard}>
        {m.settings_clash_dns_discard()}
      </Button>

      <Button
        variant="flat"
        disabled={!dirty || saving}
        loading={saving}
        onClick={handleSave}
      >
        {m.settings_clash_dns_save()}
      </Button>
    </div>
  )
}
