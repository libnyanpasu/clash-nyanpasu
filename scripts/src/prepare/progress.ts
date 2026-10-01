import type { ResolveInfo, TaskDetail } from "./types.ts";
import { normalizeVersion } from "./versions.ts";

export type TaskStatus = "Waiting" | "Pulling" | "Retrying" | "Done" | "Failed";

export interface ProgressLogger {
  start(...args: unknown[]): void;
  info(...args: unknown[]): void;
  warn(...args: unknown[]): void;
  success(...args: unknown[]): void;
}

export function formatSize(size?: number): string {
  if (size === undefined) return "";
  const units = ["B", "KB", "MB", "GB"];
  let value = size;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex++;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

export function formatSpeed(bytesPerSecond?: number): string {
  if (!bytesPerSecond) return "";
  return `${formatSize(bytesPerSecond)}/s`;
}

export function formatProgressSize(downloaded: number, total?: number): string {
  if (total === undefined) return formatSize(downloaded);
  return `${formatSize(downloaded)}/${formatSize(total)}`;
}

export function formatResolveInfo(info: ResolveInfo): TaskDetail {
  return {
    version: normalizeVersion(info.version),
    size: formatSize(info.size),
    speed: formatSpeed(info.speed),
    cached: info.cached,
    file: info.file,
  };
}

const ansi = {
  bold: (text: string) => `\x1b[1m${text}\x1b[22m`,
  gray: (text: string) => `\x1b[90m${text}\x1b[39m`,
  cyan: (text: string) => `\x1b[36m${text}\x1b[39m`,
  yellow: (text: string) => `\x1b[33m${text}\x1b[39m`,
  green: (text: string) => `\x1b[32m${text}\x1b[39m`,
  red: (text: string) => `\x1b[31m${text}\x1b[39m`,
};

export class ProgressRenderer {
  private readonly renderInterval = 120;
  private readonly width: number;
  private readonly status = new Map<
    string,
    { status: TaskStatus; detail: TaskDetail }
  >();
  private lineCount = 0;
  private rendered = false;
  private lastRenderAt = 0;
  private renderTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    names: string[],
    private readonly logger: ProgressLogger,
    private readonly enabled: boolean,
    private readonly output: (text: string) => void,
  ) {
    this.width = Math.max(...names.map((name) => name.length), 0);
    for (const name of names) {
      this.status.set(name, { status: "Waiting", detail: {} });
    }
  }

  start() {
    if (!this.enabled) {
      this.logger.start("start check and download resources...");
      return;
    }
    this.write("\x1b[?25l");
    this.render();
  }

  update(name: string, status: TaskStatus, detail: TaskDetail = {}) {
    const previous = this.status.get(name);
    this.status.set(name, { status, detail });
    if (!this.enabled) {
      if (status === "Pulling" && previous?.status === "Pulling") return;
      const detailText = this.formatDetailText(detail);
      if (status === "Pulling") {
        this.logger.info(`${name} Pulling ${detailText}`);
      }
      if (status === "Retrying") {
        this.logger.warn(`${name} Retrying ${detailText}`);
      }
      if (status === "Done") this.logger.success(`${name} Done ${detailText}`);
      return;
    }
    this.queueRender(status !== "Pulling");
  }

  finish() {
    if (!this.enabled) return;
    if (this.renderTimer) {
      clearTimeout(this.renderTimer);
      this.renderTimer = undefined;
    }
    this.render();
    this.write("\x1b[?25h");
  }

  private queueRender(immediate = false) {
    if (immediate) {
      if (this.renderTimer) {
        clearTimeout(this.renderTimer);
        this.renderTimer = undefined;
      }
      this.render();
      return;
    }
    const elapsed = performance.now() - this.lastRenderAt;
    if (elapsed >= this.renderInterval) {
      this.render();
      return;
    }
    if (this.renderTimer) return;
    this.renderTimer = setTimeout(() => {
      this.renderTimer = undefined;
      this.render();
    }, this.renderInterval - elapsed);
  }

  private render() {
    const lines = [...this.status.entries()].map(([name, { status, detail }]) =>
      `${this.formatStatus(status)} ${ansi.bold(name.padEnd(this.width))}  ${
        this.formatDetail(detail)
      }`.trimEnd()
    );
    const output = `${this.rendered ? `\x1b[${this.lineCount}A\x1b[J` : ""}${
      lines.join("\n")
    }\n`;
    this.write(output);
    this.lineCount = lines.length;
    this.rendered = true;
    this.lastRenderAt = performance.now();
  }

  private formatStatus(status: TaskStatus) {
    switch (status) {
      case "Waiting":
        return ansi.gray("Waiting".padEnd(8));
      case "Pulling":
        return ansi.cyan("Pulling".padEnd(8));
      case "Retrying":
        return ansi.yellow("Retrying".padEnd(8));
      case "Done":
        return ansi.green("Done".padEnd(8));
      case "Failed":
        return ansi.red("Failed".padEnd(8));
    }
  }

  private formatDetail(detail: TaskDetail): string {
    const version = detail.version
      ? `${ansi.gray("version")} ${ansi.yellow(detail.version.padEnd(24))}`
      : " ".repeat(32);
    const size = detail.size
      ? `${ansi.gray("size")} ${ansi.cyan(detail.size.padStart(15))}`
      : " ".repeat(20);
    const speed = detail.speed
      ? `${ansi.gray("speed")} ${ansi.cyan(detail.speed.padStart(12))}`
      : " ".repeat(18);
    const cached = detail.cached ? ansi.gray("cached") : " ".repeat(6);
    const file = detail.file ? ansi.gray(detail.file) : "";
    const note = detail.note ? ansi.gray(detail.note) : "";
    return `${version}  ${size}  ${speed}  ${cached}  ${file || note}`
      .trimEnd();
  }

  private formatDetailText(detail: TaskDetail): string {
    return [
      detail.version ? `version=${detail.version}` : "",
      detail.size ? `size=${detail.size}` : "",
      detail.speed ? `speed=${detail.speed}` : "",
      detail.cached ? "cached" : "",
      detail.file,
      detail.note,
    ].filter(Boolean).join(" ");
  }

  private write(text: string) {
    this.output(text);
  }
}
