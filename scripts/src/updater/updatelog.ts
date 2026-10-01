import * as path from "jsr:@std/path";
import { exists } from "jsr:@std/fs";

export async function resolveUpdateLog(tag: string) {
  const reTitle = /^## v[\d.]+/;
  const reEnd = /^---/;

  const file = path.join(Deno.cwd(), "UPDATELOG.md");

  if (!(await exists(file))) {
    throw new Error("could not found UPDATELOG.md");
  }

  const data = await Deno.readTextFile(file);

  const map: Record<string, string[]> = {};
  let p = "";

  data.split("\n").forEach((line) => {
    if (reTitle.test(line)) {
      p = line.slice(3).trim();
      if (!map[p]) {
        map[p] = [];
      } else {
        throw new Error(`Tag ${p} dup`);
      }
    } else if (reEnd.test(line)) {
      p = "";
    } else if (p) {
      map[p].push(line);
    }
  });

  if (!map[tag]) {
    throw new Error(`could not found "${tag}" in UPDATELOG.md`);
  }

  return map[tag].join("\n").trim();
}
