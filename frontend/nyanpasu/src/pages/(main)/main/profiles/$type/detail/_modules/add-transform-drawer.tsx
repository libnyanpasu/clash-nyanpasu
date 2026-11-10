import AddRounded from '~icons/material-symbols/add-rounded'
import CloseRounded from '~icons/material-symbols/close-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerHandle,
  DrawerTitle,
} from '@nyanpasu/ui/drawer'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { CompactButton } from '../../../_modules/compact-controls'
import { TransformLibrary } from '../../../_modules/transform-library'
import { type TransformChainDraft } from '../../../_modules/use-transform-chain-draft'

/**
 * The shared library of a narrow panel, in a bottom drawer. It stays open
 * while transforms are toggled, until it is closed.
 */
export default function AddTransformDrawer({
  chain,
}: {
  chain: TransformChainDraft
}) {
  const [open, setOpen] = useState(false)

  return (
    <>
      <CompactButton
        tone="tonal"
        data-slot="profile-chain-editor-add"
        onClick={() => setOpen(true)}
      >
        <AddRounded aria-hidden />
        {m.profile_chain_editor_add()}
      </CompactButton>

      <Drawer open={open} onOpenChange={setOpen}>
        <DrawerContent
          aria-describedby={undefined}
          className="h-[85dvh] max-h-[85dvh] min-h-0 w-[calc(100%-1rem)] max-w-[960px]"
        >
          <DrawerHandle />

          <div className="flex shrink-0 items-center justify-between gap-4 px-4 pt-2">
            <DrawerTitle className="min-w-0 truncate text-lg font-semibold">
              {m.profile_chain_editor_add()}
            </DrawerTitle>

            <DrawerClose asChild>
              <Button
                variant="raised"
                className="size-8"
                icon
                aria-label={m.common_close()}
              >
                <CloseRounded className="size-4" />
              </Button>
            </DrawerClose>
          </div>

          <ScrollArea
            data-slot="profile-chain-editor-add-scroll-area"
            className="min-h-0 flex-1"
          >
            <div className="px-3 pt-2 pb-6">
              <TransformLibrary chain={chain} />
            </div>
          </ScrollArea>
        </DrawerContent>
      </Drawer>
    </>
  )
}
