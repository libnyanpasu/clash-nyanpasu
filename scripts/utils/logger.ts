import { createConsola } from "consola";

export const consola = createConsola({
  level: process.env.LOG_LEVEL ? Number.parseInt(process.env.LOG_LEVEL) : 5,
  fancy: true,
  formatOptions: {
    columns: 80,
    colors: true,
    compact: false,
    date: true,
  },
});

consola.wrapAll();
