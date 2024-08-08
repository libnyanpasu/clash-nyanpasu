import Masonry from "@mui/lab/Masonry";
import SettingClashBase from "./setting-clash-base";
import SettingClashCore from "./setting-clash-core";
import SettingClashExternal from "./setting-clash-external";
import SettingClashField from "./setting-clash-field";
import SettingClashPort from "./setting-clash-port";
import SettingClashWeb from "./setting-clash-web";
import SettingNyanpasuMisc from "./setting-nyanpasu-misc";
import SettingNyanpasuPath from "./setting-nyanpasu-path";
import SettingNyanpasuTasks from "./setting-nyanpasu-tasks";
import SettingNyanpasuUI from "./setting-nyanpasu-ui";
import SettingNyanpasuVersion from "./setting-nyanpasu-version";
import SettingSystemBehavior from "./setting-system-behavior";
import SettingSystemProxy from "./setting-system-proxy";
import SettingSystemService from "./setting-system-service";

export const SettingPage = () => {
  return (
    <Masonry
      className="w-full"
      columns={{ xs: 1, sm: 1, md: 2 }}
      spacing={3}
      sequential
      sx={{ width: "calc(100% + 24px)" }}
    >
      <SettingSystemProxy />

      <SettingNyanpasuUI />

      <SettingClashBase />

      <SettingClashPort />

      <SettingClashExternal />

      <SettingClashWeb />

      <SettingClashField />

      <SettingClashCore />

      <SettingSystemBehavior />

      <SettingSystemService />

      <SettingNyanpasuTasks />

      <SettingNyanpasuMisc />

      <SettingNyanpasuPath />

      <SettingNyanpasuVersion />
    </Masonry>
  );
};

export default SettingPage;
