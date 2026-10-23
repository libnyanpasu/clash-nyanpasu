import { createColorize } from "colorize-template";
import { createConsola } from "consola";
import pc from "picocolors";

const logLevelStr = process.env.LOG_LEVEL;

export const consola = createConsola({
  level: logLevelStr ? Number.parseInt(logLevelStr) : 5,
  fancy: true,
  formatOptions: {
    colors: true,
    compact: false,
    date: true,
  },
});

export const colorize = createColorize({
  ...pc,
  success: pc.green,
  error: pc.red,
});
