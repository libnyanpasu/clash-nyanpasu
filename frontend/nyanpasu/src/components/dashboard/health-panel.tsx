import { CircularProgress, IconButton, Paper, Tooltip } from "@mui/material";
import Grid from "@mui/material/Unstable_Grid2";
import { timing, useIPSB } from "@nyanpasu/interface";
import { useInterval } from "ahooks";
import { useRef, useState } from "react";
import { countryCodeEmoji } from "country-code-emoji";
import { Visibility, VisibilityOff } from "@mui/icons-material";
import { cn } from "@nyanpasu/ui";
import { getColorForDelay } from "../proxies/utils";

const IP_REFRESH_SECONDS = 180;

export const HealthPanel = () => {
  const [health, setHealth] = useState({
    Google: 0,
    GitHub: 0,
    BingCN: 0,
    Baidu: 0,
  });

  const healthCache = useRef({
    Google: 0,
    GitHub: 0,
    BingCN: 0,
    Baidu: 0,
  });

  const refreshCount = useRef({
    ip: 0,
  });

  useInterval(async () => {
    setHealth(healthCache.current);

    if (refreshCount.current.ip >= IP_REFRESH_SECONDS) {
      handleRefreshIP();
    } else {
      refreshCount.current.ip++;
    }

    healthCache.current = {
      Google: await timing.Google(),
      GitHub: await timing.GitHub(),
      BingCN: await timing.BingCN(),
      Baidu: await timing.Baidu(),
    };
  }, 1000);

  const { data, mutate } = useIPSB();

  const handleRefreshIP = () => {
    refreshCount.current.ip = 0;
    mutate();
  };

  const [showIPAddress, setShowIPAddress] = useState(false);

  return (
    <Grid sm={12} md={8} lg={6} xl={4} className="w-full">
      <Paper className="!rounded-3xl relative">
        <div className="p-4 flex justify-between gap-8">
          <div className="flex flex-col justify-between">
            {Object.entries(health).map(([name, value]) => {
              return (
                <div key={name} className="flex gap-1 justify-between">
                  <div className="min-w-20 font-bold">{name}:</div>

                  <div
                    className="min-w-16 text-end"
                    style={{ color: getColorForDelay(value) }}
                  >
                    {value ? `${value.toFixed(0)} ms` : "Timeout"}
                  </div>
                </div>
              );
            })}
          </div>

          <div className="flex justify-center gap-4 flex-1 relative select-text">
            {data && (
              <>
                <div className="text-5xl relative">
                  <span className="blur opacity-50">
                    {countryCodeEmoji(data.country_code)}
                  </span>

                  <span className="absolute left-0">
                    {countryCodeEmoji(data.country_code)}
                  </span>
                </div>

                <div className="flex flex-col gap-1">
                  <div className="text-xl font-bold text-shadow-md flex justify-between items-end">
                    <div>{data.country}</div>

                    <Tooltip title="Click to Refresh Now">
                      <IconButton className="!size-8" onClick={handleRefreshIP}>
                        <CircularProgress
                          size={16}
                          variant="determinate"
                          value={
                            100 -
                            100 * (refreshCount.current.ip / IP_REFRESH_SECONDS)
                          }
                        />
                      </IconButton>
                    </Tooltip>
                  </div>

                  <div>{data.organization}</div>

                  <div className="text-sm">AS{data.asn}</div>

                  <div className="w-full flex gap-4 items-center">
                    <div className="font-mono relative">
                      <span
                        className={cn(
                          "transition-opacity",
                          showIPAddress ? "opacity-1000" : "opacity-0",
                        )}
                      >
                        {data.ip}
                      </span>

                      <span
                        className={cn(
                          "bg-slate-300 absolute w-full h-full left-0 transition-opacity rounded-lg",
                          showIPAddress
                            ? "opacity-0"
                            : "opacity-100 animate-pulse",
                        )}
                      />
                    </div>

                    <IconButton
                      className="!size-4"
                      color="primary"
                      onClick={() => setShowIPAddress(!showIPAddress)}
                    >
                      {showIPAddress ? <Visibility /> : <VisibilityOff />}
                    </IconButton>
                  </div>
                </div>
              </>
            )}
          </div>
        </div>
      </Paper>
    </Grid>
  );
};

export default HealthPanel;
