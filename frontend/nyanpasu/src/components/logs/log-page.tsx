import { useAtomValue } from "jotai";
import ContentDisplay from "../base/content-display";
import ClearLogButton from "./clear-log-button";
import { LogList } from "./log-list";
import { atomLogList } from "./modules/store";

export const LogPage = () => {
  const logList = useAtomValue(atomLogList);

  return (
    <>
      {logList?.data.length ? (
        <LogList data={logList.data} scrollRef={logList.scrollRef} />
      ) : (
        <ContentDisplay className="absolute" message="No logs" />
      )}

      <ClearLogButton />
    </>
  );
};

export default LogPage;
