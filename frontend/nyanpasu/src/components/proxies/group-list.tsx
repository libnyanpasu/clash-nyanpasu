import { proxyGroupAtom } from "@/store";
import {
  List,
  ListItem,
  ListItemButton,
  ListItemButtonProps,
  ListItemIcon,
  ListItemText,
} from "@mui/material";
import { useClashCore } from "@nyanpasu/interface";
import { useAtom } from "jotai";
import { memo } from "react";

const IconRender = memo(function IconRender({ icon }: { icon: string }) {
  const src = icon.trim().startsWith("<svg")
    ? `data:image/svg+xml;base64,${btoa(icon)}`
    : icon;

  return (
    <ListItemIcon>
      <img className="w-11 h-11" src={src} />
    </ListItemIcon>
  );
});

export const GroupList = (listItemButtonProps: ListItemButtonProps) => {
  const { data } = useClashCore();

  const [proxyGroup, setProxyGroup] = useAtom(proxyGroupAtom);

  const handleSelect = (index: number) => {
    setProxyGroup({ selector: index });
  };

  return (
    <List disablePadding>
      {data?.groups?.map((group, index) => {
        return (
          <ListItem key={index} disablePadding>
            <ListItemButton
              selected={index === proxyGroup.selector}
              onClick={() => handleSelect(index)}
              {...listItemButtonProps}
            >
              {group.icon && <IconRender icon={group.icon} />}

              <ListItemText primary={group.name} secondary={group.now} />
            </ListItemButton>
          </ListItem>
        );
      })}
    </List>
  );
};
