export interface ClashManifest {
  URL_PREFIX: string;
  LATEST_DATE?: string;
  STORAGE_PREFIX?: string;
  BACKUP_URL_PREFIX?: string;
  BACKUP_LATEST_DATE?: string;
  VERSION?: string;
  VERSION_URL?: string;
  BIN_MAP: { [key: string]: string };
}

export interface BinInfo {
  name: string;
  targetFile: string;
  exeFile: string;
  tmpFile: string;
  downloadURL: string;
}
