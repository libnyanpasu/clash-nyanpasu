import parseTraffic from "@/utils/parse-traffic";
import { GridColDef, DataGrid } from "@mui/x-data-grid";
import { useClashWS, Connection } from "@nyanpasu/interface";
import dayjs from "dayjs";
import { useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import HeaderSearch from "./header-search";

export type TableConnection = Connection.Item &
  Connection.Metadata & {
    downloadSpeed?: number;
    uploadSpeed?: number;
  };

export interface TableMessage extends Omit<Connection.Response, "connections"> {
  connections: TableConnection[];
}

export const ConnectionsTable = () => {
  const { t } = useTranslation();

  const {
    connections: { latestMessage },
  } = useClashWS();

  const historyMessage = useRef<TableMessage | undefined>(undefined);

  const connectionsMessage = useMemo(() => {
    if (!latestMessage?.data) return;

    const result = JSON.parse(latestMessage.data) as Connection.Response;

    const updatedConnections: TableConnection[] = [];

    result.connections?.forEach((connection) => {
      const previousConnection = historyMessage.current?.connections.find(
        (history) => history.id === connection.id,
      );

      const downloadSpeed = previousConnection
        ? connection.download - previousConnection.download
        : 0;

      const uploadSpeed = previousConnection
        ? connection.upload - previousConnection.upload
        : 0;

      updatedConnections.push({
        ...connection,
        ...connection.metadata,
        downloadSpeed,
        uploadSpeed,
      });
    });

    const data = { ...result, connections: updatedConnections };

    historyMessage.current = data;

    return data;
  }, [latestMessage?.data]);

  const columns: GridColDef[] = [
    {
      field: "host",
      headerName: t("Host"),
      flex: 240,
      minWidth: 240,
    },
    {
      field: "process",
      headerName: t("Process"),
      flex: 100,
      minWidth: 100,
    },
    {
      field: "download",
      headerName: t("Download"),
      width: 88,
      valueFormatter: (value) => parseTraffic(value).join(" "),
    },
    {
      field: "upload",
      headerName: t("Upload"),
      width: 88,
      valueFormatter: (value) => parseTraffic(value).join(" "),
    },
    {
      field: "downloadSpeed",
      headerName: t("DL Speed"),
      width: 88,
      valueFormatter: (value) => parseTraffic(value).join(" ") + "/s",
    },
    {
      field: "uploadSpeed",
      headerName: t("UL Speed"),
      width: 88,
      valueFormatter: (value) => parseTraffic(value).join(" ") + "/s",
    },
    {
      field: "chains",
      headerName: t("Chains"),
      flex: 360,
      minWidth: 360,
      valueFormatter: (value) => [...value].reverse().join(" / "),
    },
    {
      field: "rule",
      headerName: "Rule",
      flex: 300,
      minWidth: 250,
    },
    {
      field: "start",
      headerName: t("Time"),
      flex: 120,
      minWidth: 100,
      valueFormatter: (value) => dayjs(value).fromNow(),
    },
    {
      field: "source",
      headerName: "Source",
      flex: 200,
      minWidth: 130,
    },
    {
      field: "destinationIP",
      headerName: t("Destination IP"),
      flex: 200,
      minWidth: 130,
    },
    {
      field: "type",
      headerName: t("Type"),
      flex: 160,
      minWidth: 100,
    },
  ];

  return (
    connectionsMessage?.connections && (
      <DataGrid
        rows={connectionsMessage.connections}
        columns={columns}
        density="compact"
        autosizeOnMount
        hideFooter
        disableColumnFilter
        disableColumnSelector
        disableDensitySelector
        sx={{ border: "none", "div:focus": { outline: "none !important" } }}
        className="!absolute !h-full !w-full"
        slots={{
          toolbar: HeaderSearch,
        }}
      />
    )
  );
};

export default ConnectionsTable;
