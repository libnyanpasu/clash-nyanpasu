import { assertEquals } from "jsr:@std/assert@1";
import {
  affinityMask,
  chooseProcessors,
  parseCpuList,
  type Processor,
} from "./cpu-cores.ts";

// Two performance cores with two threads each, then two efficiency cores, as
// a hybrid Intel part reports them.
const HYBRID: Processor[] = [
  { processor: 0, core: 0, efficiency: 1 },
  { processor: 1, core: 0, efficiency: 1 },
  { processor: 2, core: 2, efficiency: 1 },
  { processor: 3, core: 2, efficiency: 1 },
  { processor: 4, core: 4, efficiency: 0 },
  { processor: 5, core: 5, efficiency: 0 },
];

Deno.test("chooseProcessors takes one thread per core of the kind", () => {
  assertEquals(chooseProcessors(HYBRID, "performance"), [0, 2]);
  assertEquals(chooseProcessors(HYBRID, "efficiency"), [4, 5]);
});

Deno.test("chooseProcessors takes every core of a single class", () => {
  const uniform = HYBRID.map((processor) => ({ ...processor, efficiency: 0 }));

  assertEquals(chooseProcessors(uniform, "performance"), [0, 2, 4, 5]);
  assertEquals(chooseProcessors(uniform, "efficiency"), [0, 2, 4, 5]);
});

Deno.test("affinityMask sets a bit per processor, past 32 as well", () => {
  assertEquals(affinityMask([0, 2, 4, 6]), "55");
  assertEquals(affinityMask([16, 31]), "80010000");
  assertEquals(affinityMask([40]), "10000000000");
});

Deno.test("parseCpuList expands ranges and single processors", () => {
  assertEquals(parseCpuList("0-3,8,10-11\n"), [0, 1, 2, 3, 8, 10, 11]);
  assertEquals(parseCpuList(""), []);
});
