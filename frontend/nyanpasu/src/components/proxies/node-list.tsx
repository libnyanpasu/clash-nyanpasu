import {
  Box,
  Chip,
  ChipProps,
  CircularProgress,
  useTheme,
} from "@mui/material";
import Grid from "@mui/material/Unstable_Grid2";
import { PaperSwitchButton } from "../setting/modules/system-proxy";
import { Clash, useClashCore, useNyanpasu } from "@nyanpasu/interface";
import { useAtom } from "jotai";
import { proxyGroupAtom } from "@/store";
import { memo, useMemo, useState } from "react";
import { classNames } from "@/utils";

type History = Clash.Proxy["history"];

const filterDelay = (history?: History): number => {
  if (!history || history.length == 0) {
    return 0;
  } else {
    return history[history.length - 1].delay;
  }
};

const getColorForDelay = (delay: number): string => {
  const { palette } = useTheme();

  const delayColorMapping: { [key: string]: string } = {
    "0": palette.text.secondary,
    "100": palette.success.main,
    "500": palette.warning.main,
    "1000": palette.error.main,
  };

  let color: string = palette.text.secondary;

  for (const key in delayColorMapping) {
    if (delay <= parseInt(key)) {
      color = delayColorMapping[key];
      break;
    }
  }

  return color;
};

const FeatureChip = memo(function FeatureChip(props: ChipProps) {
  return (
    <Chip
      variant="outlined"
      size="small"
      {...props}
      sx={{
        fontSize: 10,
        height: 16,
        padding: 0,

        "& .MuiChip-label": {
          padding: "0 4px",
        },
        ...props.sx,
      }}
    />
  );
});

const DelayChip = memo(function DelayChip({
  delay,
  onClick,
}: {
  delay: number;
  onClick: () => Promise<void>;
}) {
  const [loading, setLoading] = useState(false);

  const handleClick = async () => {
    try {
      setLoading(true);

      await onClick();
    } finally {
      setLoading(false);
    }
  };

  return (
    <FeatureChip
      sx={{
        ml: "auto",
        color: getColorForDelay(delay),
      }}
      label={
        <>
          <span
            className={classNames(
              "transition-opacity",
              loading ? "opacity-0" : "opacity-1",
            )}
          >
            {`${delay} ms`}
          </span>

          <CircularProgress
            size={12}
            className={classNames(
              "transition-opacity",
              "absolute",
              "animate-spin",
              "top-0",
              "bottom-0",
              "left-0",
              "right-0",
              "m-auto",
              loading ? "opacity-1" : "opacity-0",
            )}
          />
        </>
      }
      variant="filled"
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        handleClick();
      }}
    />
  );
});

const NodeCard = memo(function NodeCard({
  node,
  now,
  disabled,
  onClick,
  onClickDelay,
}: {
  node: Clash.Proxy<string>;
  now?: string;
  disabled?: boolean;
  onClick: () => void;
  onClickDelay: () => Promise<void>;
}) {
  const delay = useMemo(() => filterDelay(node.history), [node.history]);

  return (
    <PaperSwitchButton
      label={node.name}
      checked={node.name === now}
      onClick={onClick}
      disabled={disabled}
    >
      <Box width="100%" display="flex" gap={0.5}>
        <FeatureChip label={node.type} />

        {node.udp && <FeatureChip label="UDP" />}

        <DelayChip delay={delay} onClick={onClickDelay} />
      </Box>
    </PaperSwitchButton>
  );
});

export const NodeList = () => {
  const { data, setGroupProxy, updateProxiesDelay } = useClashCore();

  const { getCurrentMode } = useNyanpasu();

  const [proxyGroup] = useAtom(proxyGroupAtom);

  const group = useMemo(() => {
    if (getCurrentMode.global) {
      return data?.global;
    } else {
      if (proxyGroup.selector !== null) {
        return data?.groups[proxyGroup.selector];
      } else {
        return undefined;
      }
    }
  }, [data?.groups, proxyGroup.selector, getCurrentMode]);

  const hendleClick = (node: string) => {
    setGroupProxy(proxyGroup.selector as number, node);
  };

  return (
    <Box sx={{ padding: 2 }}>
      <Grid container spacing={2}>
        {group?.all?.map((node, index) => {
          return (
            <Grid key={index} xs={12} sm={6} lg={4} xl={3}>
              <NodeCard
                node={node}
                now={group.now}
                disabled={group.type !== "Selector"}
                onClick={() => hendleClick(node.name)}
                onClickDelay={async () => {
                  await updateProxiesDelay(node.name);
                }}
              />
            </Grid>
          );
        })}
      </Grid>
    </Box>
  );
};
