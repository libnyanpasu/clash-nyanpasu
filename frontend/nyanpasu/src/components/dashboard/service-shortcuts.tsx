import { useAtomValue } from "jotai";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { atomIsDrawer } from "@/store";
import { alpha, CircularProgress, Paper, useTheme } from "@mui/material";
import Grid from "@mui/material/Unstable_Grid2";
import { useClash, useNyanpasu } from "@nyanpasu/interface";

export const ServiceShortcuts = () => {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const isDrawer = useAtomValue(atomIsDrawer);

  const {
    getServiceStatus: { data: serviceStatus },
  } = useNyanpasu();

  const {
    getVersion: { data: coreVersion },
  } = useClash();

  const status = useMemo(() => {
    switch (serviceStatus) {
      case "running": {
        return {
          label: "Running",
          color: alpha(palette.success[palette.mode], 0.3),
        };
      }

      case "stopped": {
        return {
          label: "Stopped",
          color: alpha(palette.error[palette.mode], 0.3),
        };
      }

      default:
      case "not_installed": {
        return {
          label: "Not Installed",
          color:
            palette.mode == "light"
              ? palette.grey[100]
              : palette.background.paper,
        };
      }
    }
  }, [serviceStatus, palette]);

  const coreStatus = useMemo(() => {
    if (coreVersion) {
      if (serviceStatus == "running") {
        return {
          label: "Start by Service",
          color: alpha(palette.success[palette.mode], 0.3),
        };
      } else {
        return {
          label: "Start by UI",
          color: alpha(palette.success[palette.mode], 0.3),
        };
      }
    } else {
      return {
        label: "Clash Core did not start",
        color: alpha(palette.error.main, 0.3),
      };
    }
  }, [coreVersion, serviceStatus, palette]);

  return (
    <Grid sm={isDrawer ? 6 : 12} md={6} lg={4} xl={3}>
      <Paper className="flex !h-full flex-col justify-between gap-2 !rounded-3xl p-3">
        {serviceStatus ? (
          <>
            <div className="text-center font-bold">Service Shortcuts</div>

            <div className="flex w-full flex-col gap-2">
              <div
                className="flex w-full justify-center gap-2 rounded-2xl py-2"
                style={{ backgroundColor: status.color }}
              >
                <div>Service Status:</div>
                <div>{status.label}</div>
              </div>

              <div
                className="flex w-full justify-center gap-2 rounded-2xl py-2"
                style={{ backgroundColor: coreStatus.color }}
              >
                <div>Core Status:</div>
                <div>{coreStatus.label}</div>
              </div>
            </div>
          </>
        ) : (
          <div className="flex w-full flex-col items-center justify-center gap-2">
            <CircularProgress />

            <div>Loading...</div>
          </div>
        )}
      </Paper>
    </Grid>
  );
};

export default ServiceShortcuts;
