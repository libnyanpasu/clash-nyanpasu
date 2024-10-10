import { useLockFn } from "ahooks";
import * as changeCase from "change-case";
import dayjs from "dayjs";
import { t } from "i18next";
import { size } from "lodash-es";
import {
  MaterialReactTable,
  useMaterialReactTable,
  type MRT_ColumnDef,
} from "material-react-table";
import { useDeferredValue, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { containsSearchTerm } from "@/utils";
import parseTraffic from "@/utils/parse-traffic";
import Cancel from "@mui/icons-material/Cancel";
import { IconButton } from "@mui/material";
import { Connection, useClash, useClashWS } from "@nyanpasu/interface";
import ContentDisplay from "../base/content-display";

export type TableConnection = Connection.Item & {
  downloadSpeed?: number;
  uploadSpeed?: number;
};

export interface TableMessage extends Omit<Connection.Response, "connections"> {
  connections: TableConnection[];
}

export const ConnectionsTable = ({ searchTerm }: { searchTerm?: string }) => {
  const { t } = useTranslation();

  const { deleteConnections } = useClash();

  const closeConnect = useLockFn(async (id?: string) => {
    await deleteConnections(id);
  });

  const {
    connections: { latestMessage },
  } = useClashWS();

  const historyMessage = useRef<TableMessage | undefined>(undefined);

  const connectionsMessage = useMemo(() => {
    if (!latestMessage?.data) return;

    const result = JSON.parse(latestMessage.data) as Connection.Response;

    const updatedConnections: TableConnection[] = [];

    const filteredConnections = searchTerm
      ? result.connections?.filter((connection) =>
          containsSearchTerm(connection, searchTerm),
        )
      : result.connections;

    filteredConnections?.forEach((connection) => {
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
        downloadSpeed,
        uploadSpeed,
      });
    });

    const data = { ...result, connections: updatedConnections };

    historyMessage.current = data;

    return data;
  }, [latestMessage?.data, searchTerm]);
  const deferredTableData = useDeferredValue(connectionsMessage?.connections);

  const columns = useMemo(
    () =>
      (
        [
          {
            header: "Actions",
            size: 80,
            enableSorting: false,
            enableGlobalFilter: false,
            accessorFn: ({ id }) => (
              <div className="flex w-full justify-center">
                <IconButton
                  color="primary"
                  className="size-5"
                  onClick={() => closeConnect(id)}
                >
                  <Cancel />
                </IconButton>
              </div>
            ),
          },
          {
            header: "Host",
            size: 240,
            accessorFn: ({ metadata }) =>
              metadata.host || metadata.destinationIP,
          },
          {
            header: "Process",
            size: 140,
            accessorFn: ({ metadata }) => metadata.process,
          },
          {
            header: "Downloaded",
            size: 88,
            accessorFn: ({ download }) => parseTraffic(download).join(" "),
            sortingFn: (rowA, rowB) =>
              rowB.original.download - rowA.original.download,
          },
          {
            header: "Uploaded",
            size: 88,
            accessorFn: ({ upload }) => parseTraffic(upload).join(" "),
            sortingFn: (rowA, rowB) =>
              rowB.original.upload - rowA.original.upload,
          },
          {
            header: "DL Speed",
            size: 88,
            accessorFn: ({ downloadSpeed }) =>
              parseTraffic(downloadSpeed).join(" ") + "/s",
            sortingFn: (rowA, rowB) =>
              (rowA.original.downloadSpeed || 0) -
              (rowB.original.downloadSpeed || 0),
          },
          {
            header: "UL Speed",
            size: 88,
            accessorFn: ({ uploadSpeed }) =>
              parseTraffic(uploadSpeed).join(" ") + "/s",
            sortingFn: (rowA, rowB) =>
              (rowA.original.uploadSpeed || 0) -
              (rowB.original.uploadSpeed || 0),
          },
          {
            header: "Chains",
            size: 360,
            accessorFn: ({ chains }) => [...chains].reverse().join(" / "),
          },
          {
            header: "Rules",
            size: 200,
            accessorFn: ({ rule, rulePayload }) =>
              rulePayload ? `${rule} (${rulePayload})` : rule,
          },
          {
            header: "Time",
            size: 120,
            accessorFn: ({ start }) => dayjs(start).fromNow(),
            sortingFn: (rowA, rowB) =>
              dayjs(rowB.original.start).diff(rowA.original.start),
          },
          {
            header: "Source",
            size: 200,
            accessorFn: ({ metadata: { sourceIP, sourcePort } }) =>
              `${sourceIP}:${sourcePort}`,
          },
          {
            header: "Destination",
            size: 200,
            accessorFn: ({ metadata: { destinationIP, destinationPort } }) =>
              `${destinationIP}:${destinationPort}`,
          },
          {
            header: "Type",
            size: 160,
            accessorFn: ({ metadata }) =>
              `${metadata.type} (${metadata.network})`,
          },
        ] satisfies Array<MRT_ColumnDef<TableConnection>>
      ).map(
        (column) =>
          ({
            ...column,
            id: changeCase.snakeCase(column.header),
            Header(props) {
              return <span>{t(column.header)}</span>;
            },
          }) satisfies MRT_ColumnDef<TableConnection>,
      ),
    [t],
  );

  const table = useMaterialReactTable({
    columns,
    data: deferredTableData ?? [],
    initialState: {
      density: "compact",
    },
    defaultDisplayColumn: {
      enableResizing: true,
    },
    enableTopToolbar: false,
    enableColumnActions: false,
    enablePagination: false,
    enableBottomToolbar: false,
    enableColumnResizing: true,
    enableGlobalFilterModes: true,
    enableColumnPinning: true,
    muiTableContainerProps: {
      sx: { minHeight: "100%" },
      className: "!absolute !h-full !w-full",
    },
    enableRowVirtualization: true,
    enableColumnVirtualization: true,
    rowVirtualizerOptions: { overscan: 5 },
    columnVirtualizerOptions: { overscan: 2 },
  });

  return connectionsMessage?.connections.length ? (
    <MaterialReactTable table={table} />
  ) : (
    <ContentDisplay
      className="!absolute !h-full !w-full"
      message={t("No Connections")}
    />
  );
};

export default ConnectionsTable;
