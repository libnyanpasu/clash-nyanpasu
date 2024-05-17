import parseTraffic from "@/utils/parse-traffic";
import { Update, MoreVert } from "@mui/icons-material";
import { Paper, Button, LinearProgress } from "@mui/material";
import { Profile } from "@nyanpasu/interface";
import dayjs from "dayjs";
import { memo } from "react";
import Marquee from "react-fast-marquee";

export interface ProfileItemProps {
  item: Profile.Item;
}

export const ProfileItem = memo(function ProfileItem({
  item,
}: ProfileItemProps) {
  const calc = () => {
    let progress = 0;
    let total = 0;
    let used = 0;

    if (item.extra) {
      const { download, upload, total: t } = item.extra;

      total = t;

      used = download + upload;

      progress = (used / total) * 100;
    }

    return { progress, total, used };
  };

  const { progress, total, used } = calc();

  return (
    <Paper className="p-6 flex flex-col gap-2" sx={{ borderRadius: 6 }}>
      <div className="flex justify-between">
        <div className="text-xl font-bold">{item.name}</div>

        <div className="flex gap-2">
          <Button className="size-7 !min-w-0">
            <Update />
          </Button>

          <Button className="size-7 !min-w-0">
            <MoreVert />
          </Button>
        </div>
      </div>

      <Marquee>
        <div className="text-sm pr-16">{item.url}</div>
      </Marquee>

      <div className="flex flex-row-reverse justify-between">
        <div className="text-sm">
          {item.updated! > 0 ? dayjs(item.updated! * 1000).fromNow() : ""}
        </div>

        <div className="text-sm">
          {`${parseTraffic(used)} / ${parseTraffic(total)}`}
        </div>
      </div>

      <LinearProgress variant="determinate" value={progress} />
    </Paper>
  );
});

export default ProfileItem;
