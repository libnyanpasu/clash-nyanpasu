import { useAtom } from "jotai";
import { memo, useMemo } from "react";
import useSWR from "swr";
import { Virtualizer } from "virtua";
import { proxyGroupAtom } from "@/store";
import {
  ListItem,
  ListItemButton,
  ListItemButtonProps,
  ListItemIcon,
  ListItemText,
} from "@mui/material";
import { getServerPort, useClashCore } from "@nyanpasu/interface";
import { LazyImage } from "@nyanpasu/ui";

const IconRender = memo(function IconRender({ icon }: { icon: string }) {
  const {
    data: serverPort,
    isLoading,
    error,
  } = useSWR("/getServerPort", getServerPort);
  const src = icon.trim().startsWith("<svg")
    ? `data:image/svg+xml;base64,${btoa(icon)}`
    : icon;
  const cachedUrl = useMemo(() => {
    if (!src.startsWith("http")) {
      return src;
    }
    return `http://localhost:${serverPort}/cache/icon?url=${btoa(src)}`;
  }, [src, serverPort]);
  if (isLoading || error) {
    return null;
  }
  return (
    <ListItemIcon>
      <LazyImage
        className="h-11 w-11"
        loadingClassName="rounded-full"
        src={cachedUrl}
      />
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
    <Virtualizer>
      {data?.groups?.map((group, index) => {
        return (
          <ListItem key={index} disablePadding>
            <ListItemButton
              selected={index === proxyGroup.selector}
              onClick={() => handleSelect(index)}
              {...listItemButtonProps}
            >
              {group.icon && <IconRender icon={group.icon} />}

              <ListItemText
                className="!truncate"
                primary={group.name}
                secondary={group.now}
              />
            </ListItemButton>
          </ListItem>
        );
      })}
    </Virtualizer>
  );
};
