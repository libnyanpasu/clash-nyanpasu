import { Box, ButtonBase, Chip, useTheme } from "@mui/material";
import Grid from "@mui/material/Unstable_Grid2";
import { PaperSwitchButton } from "../setting/modules/system-proxy";
import { Clash, useClashCore } from "@nyanpasu/interface";
import { useAtom } from "jotai";
import { proxyGroupAtom } from "@/store";
import { memo, useMemo } from "react";

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

const FeatureChip = memo(function FeatureChip({ label }: { label: string }) {
  return (
    <Chip
      sx={{
        fontSize: 10,
        height: 16,
        padding: 0,

        "& .MuiChip-label": {
          padding: "0 4px",
        },
      }}
      label={label}
      size="small"
      variant="outlined"
    />
  );
});

const NodeCard = memo(function NodeCard({
  node,
  now,
  onClick,
}: {
  node: Clash.Proxy<string>;
  now?: string;
  onClick: () => void;
}) {
  const delay = useMemo(() => filterDelay(node.history), [node.history]);

  return (
    <PaperSwitchButton
      label={node.name}
      checked={node.name === now}
      onClick={onClick}
    >
      <Box width="100%" display="flex" gap={0.5}>
        <FeatureChip label={node.type} />

        {node.udp && <FeatureChip label="UDP" />}

        <ButtonBase
          sx={{
            fontSize: 10,
            height: 16,
            borderRadius: 4,
            padding: "4px 8px",
            ml: "auto",
            color: getColorForDelay(delay),
          }}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
        >
          {delay} ms
        </ButtonBase>
      </Box>
    </PaperSwitchButton>
  );
});

export const NodeList = () => {
  const { data, setGroupProxy } = useClashCore();

  const [proxyGroup] = useAtom(proxyGroupAtom);

  const group = useMemo(() => {
    if (proxyGroup.selector !== null) {
      return data?.groups[proxyGroup.selector];
    } else {
      return undefined;
    }
  }, [data?.groups, proxyGroup.selector]);

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
                onClick={() => hendleClick(node.name)}
              />
            </Grid>
          );
        })}
      </Grid>
    </Box>
  );
};
