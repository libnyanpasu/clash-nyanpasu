import useSWR from "swr";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { IconButton } from "@mui/material";
import { ArrowForward, PrivacyTipRounded, Settings } from "@mui/icons-material";
import { checkService } from "@/services/cmds";
import { useVerge } from "@/hooks/use-verge";
import { DialogRef } from "@/components/base";
import { SettingList, SettingItem } from "./mods/setting-comp";
import { GuardState } from "./mods/guard-state";
import { ServiceViewer } from "./mods/service-viewer";
import { SysproxyViewer } from "./mods/sysproxy-viewer";
import getSystem from "@/utils/get-system";
import MDYSwitch from "../common/mdy-switch";

interface Props {
  onError?: (err: Error) => void;
}

const isWIN = getSystem() === "windows";

const SettingSystem = ({ onError }: Props) => {
  const { t } = useTranslation();

  const { verge, patchVerge } = useVerge();

  // service mode
  const { data: serviceStatus } = useSWR(
    isWIN ? "checkService" : null,
    checkService,
    {
      revalidateIfStale: false,
      shouldRetryOnError: false,
      focusThrottleInterval: 36e5, // 1 hour
    },
  );

  const serviceRef = useRef<DialogRef>(null);
  const sysproxyRef = useRef<DialogRef>(null);

  const {
    enable_tun_mode,
    enable_auto_launch,
    enable_service_mode,
    enable_silent_start,
    enable_system_proxy,
  } = verge ?? {};

  const [loading, setLoading] = useState({
    enable_tun_mode: false,
    enable_auto_launch: false,
    enable_service_mode: false,
    enable_silent_start: false,
    enable_system_proxy: false,
  });

  const patchVergeWithLoading = async (value: Partial<IVergeConfig>) => {
    try {
      setLoading((prevLoading) => ({
        ...prevLoading,
        ...Object.fromEntries(Object.keys(value).map((key) => [key, true])),
      }));

      await patchVerge(value);
    } finally {
      setLoading((prevLoading) => ({
        ...prevLoading,
        ...Object.fromEntries(Object.keys(value).map((key) => [key, false])),
      }));
    }
  };

  const onSwitchFormat = (_e: any, value: boolean) => value;

  return (
    <SettingList title={t("System Setting")}>
      <SysproxyViewer ref={sysproxyRef} />
      {isWIN && (
        <ServiceViewer ref={serviceRef} enable={!!enable_service_mode} />
      )}

      <SettingItem label={t("Tun Mode")}>
        <GuardState
          value={enable_tun_mode ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onGuard={(e) => patchVergeWithLoading({ enable_tun_mode: e })}
          loading={loading["enable_tun_mode"]}
        >
          <MDYSwitch edge="end" />
        </GuardState>
      </SettingItem>

      {isWIN && (
        <SettingItem
          label={t("Service Mode")}
          extra={
            <IconButton
              color="inherit"
              size="small"
              onClick={() => serviceRef.current?.open()}
            >
              <PrivacyTipRounded
                fontSize="inherit"
                style={{ cursor: "pointer", opacity: 0.75 }}
              />
            </IconButton>
          }
        >
          <GuardState
            value={enable_service_mode ?? false}
            valueProps="checked"
            onCatch={onError}
            onFormat={onSwitchFormat}
            onGuard={(e) => patchVergeWithLoading({ enable_service_mode: e })}
            loading={loading["enable_service_mode"]}
          >
            <MDYSwitch
              edge="end"
              disabled={
                serviceStatus !== "active" && serviceStatus !== "installed"
              }
            />
          </GuardState>
        </SettingItem>
      )}

      <SettingItem
        label={t("System Proxy")}
        extra={
          <IconButton
            color="inherit"
            size="small"
            onClick={() => sysproxyRef.current?.open()}
          >
            <Settings
              fontSize="inherit"
              style={{ cursor: "pointer", opacity: 0.75 }}
            />
          </IconButton>
        }
      >
        <GuardState
          value={enable_system_proxy ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onGuard={(e) => patchVergeWithLoading({ enable_system_proxy: e })}
          loading={loading["enable_system_proxy"]}
        >
          <MDYSwitch edge="end" />
        </GuardState>
      </SettingItem>

      <SettingItem label={t("Auto Launch")}>
        <GuardState
          value={enable_auto_launch ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onGuard={(e) => patchVergeWithLoading({ enable_auto_launch: e })}
          loading={loading["enable_auto_launch"]}
        >
          <MDYSwitch edge="end" />
        </GuardState>
      </SettingItem>

      <SettingItem label={t("Silent Start")}>
        <GuardState
          value={enable_silent_start ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onGuard={(e) => patchVergeWithLoading({ enable_silent_start: e })}
          loading={loading["enable_auto_launch"]}
        >
          <MDYSwitch edge="end" />
        </GuardState>
      </SettingItem>
    </SettingList>
  );
};

export default SettingSystem;
