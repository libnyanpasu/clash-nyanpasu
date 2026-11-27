import { ComponentType, createContext, PropsWithChildren, use } from 'react'
import {
  DropdownMenuCheckboxItem,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import {
  MenubarCheckboxItem,
  MenubarGroup,
  MenubarItem,
  MenubarRadioGroup,
  MenubarRadioItem,
  MenubarSeparator,
  MenubarSub,
  MenubarSubContent,
  MenubarSubTrigger,
} from '@nyanpasu/ui/menubar'

export interface HeaderMenuPrimitives {
  mode: 'menubar' | 'launcher'
  Group: ComponentType<PropsWithChildren<{ className?: string }>>
  Item: ComponentType<
    PropsWithChildren<{
      asChild?: boolean
      className?: string
      disabled?: boolean
      onSelect?: (event: Event) => void
    }>
  >
  CheckboxItem: ComponentType<
    PropsWithChildren<{
      className?: string
      checked?: boolean | 'indeterminate'
      disabled?: boolean
      title?: string
      'data-loading'?: string
      onCheckedChange?: (checked: boolean) => void
    }>
  >
  RadioGroup: ComponentType<
    PropsWithChildren<{
      value?: string
      onValueChange?: (value: string) => void
    }>
  >
  RadioItem: ComponentType<
    PropsWithChildren<{
      value: string
      disabled?: boolean
      className?: string
    }>
  >
  Sub: ComponentType<PropsWithChildren>
  SubTrigger: ComponentType<PropsWithChildren<{ className?: string }>>
  SubContent: ComponentType<PropsWithChildren<{ className?: string }>>
  Separator: ComponentType<{ className?: string }>
}

export const MENUBAR_PRIMITIVES: HeaderMenuPrimitives = {
  mode: 'menubar',
  Group: MenubarGroup,
  Item: MenubarItem,
  CheckboxItem: MenubarCheckboxItem,
  RadioGroup: MenubarRadioGroup,
  RadioItem: MenubarRadioItem,
  Sub: MenubarSub,
  SubTrigger: MenubarSubTrigger,
  SubContent: MenubarSubContent,
  Separator: MenubarSeparator,
}

export const LAUNCHER_MENUBAR_PRIMITIVES: HeaderMenuPrimitives = {
  ...MENUBAR_PRIMITIVES,
  mode: 'launcher',
}

export const LAUNCHER_PRIMITIVES: HeaderMenuPrimitives = {
  mode: 'launcher',
  Group: DropdownMenuGroup,
  Item: DropdownMenuItem,
  CheckboxItem: DropdownMenuCheckboxItem,
  RadioGroup: DropdownMenuRadioGroup,
  RadioItem: DropdownMenuRadioItem,
  Sub: DropdownMenuSub,
  SubTrigger: DropdownMenuSubTrigger,
  SubContent: DropdownMenuSubContent,
  Separator: DropdownMenuSeparator,
}

const HeaderMenuPrimitivesContext =
  createContext<HeaderMenuPrimitives>(MENUBAR_PRIMITIVES)

export const HeaderMenuPrimitivesProvider = HeaderMenuPrimitivesContext.Provider

export const useHeaderMenuPrimitives = () => use(HeaderMenuPrimitivesContext)
