// @ts-types="npm:@types/d3@7.4.3"
import {
  geoGraticule10,
  geoNaturalEarth1,
  geoPath,
  type GeoPermissibleObjects,
} from "npm:d3@7.9.0";
import { format } from "npm:prettier@3.9.9";
import { dirname, join } from "jsr:@std/path";
import { WORKSPACE_ROOT } from "../shared/repo-paths.ts";

type LabelCollection = {
  features: Array<
    { properties: { ISO_A2_EH: string; LABEL_X: number; LABEL_Y: number } }
  >;
};

const revision = "ca96624a56bd078437bca8184e78163e5039ad19";
const source =
  `https://raw.githubusercontent.com/nvkelso/natural-earth-vector/${revision}/geojson`;
async function downloadGeoJson<T>(scale: number): Promise<T> {
  const response = await fetch(
    `${source}/ne_${scale}m_admin_0_countries.geojson`,
  );
  if (!response.ok) {
    throw new Error(`Natural Earth download failed: ${response.status}`);
  }
  return await response.json() as T;
}

const [countries, locations] = await Promise.all([
  downloadGeoJson<GeoPermissibleObjects>(110),
  downloadGeoJson<LabelCollection>(10),
]);
const projection = geoNaturalEarth1().fitExtent([[18, 18], [942, 442]], {
  type: "Sphere",
});
const path = geoPath(projection).digits(1);
const centers: Record<string, number[]> = {};
for (const { properties: p } of locations.features) {
  const code = p.ISO_A2_EH;
  if (!/^[A-Z]{2}$/.test(code)) continue;
  centers[code] = projection([p.LABEL_X, p.LABEL_Y])!.map((value) =>
    Math.round(value * 10) / 10
  );
}
const output = join(
  WORKSPACE_ROOT,
  "frontend/nyanpasu/src/assets/maps/world-map.json",
);
await Deno.mkdir(dirname(output), { recursive: true });
await Deno.writeTextFile(
  output,
  await format(
    JSON.stringify({
      outline: path(countries),
      graticule: path(geoGraticule10()),
      centers: Object.fromEntries(Object.entries(centers).sort()),
    }),
    { parser: "json" },
  ),
);
console.log(
  `Generated offline map with ${Object.keys(centers).length} region labels`,
);
