/** A logical processor, its physical core and its efficiency class. */
export type Processor = { processor: number; core: number; efficiency: number };

export type CoreKind = "performance" | "efficiency";

/**
 * One logical processor per physical core of the given kind: the cores of
 * the highest efficiency class for `performance`, of the lowest for
 * `efficiency`. A processor without hybrid cores has a single class, so both
 * kinds pick every core. Two threads of one core would share its units, so
 * only the first of each is taken.
 */
export function chooseProcessors(processors: Processor[], kind: CoreKind) {
  const classes = processors.map(({ efficiency }) => efficiency);
  const target = kind === "performance"
    ? Math.max(...classes)
    : Math.min(...classes);
  const firstOfCore = new Map<number, number>();

  for (const { processor, core, efficiency } of processors) {
    if (efficiency !== target) continue;
    const first = firstOfCore.get(core);
    if (first === undefined || processor < first) {
      firstOfCore.set(core, processor);
    }
  }

  return [...firstOfCore.values()].sort((a, b) => a - b);
}

/** The affinity mask of the processors, in hexadecimal. */
export function affinityMask(processors: number[]) {
  return processors
    .reduce((mask, processor) => mask | (1n << BigInt(processor)), 0n)
    .toString(16);
}

/** Reads the CPU sets Windows reports, from the first processor group. */
function windowsProcessors(): Processor[] {
  const kernel32 = Deno.dlopen("kernel32.dll", {
    GetSystemCpuSetInformation: {
      parameters: ["buffer", "u32", "buffer", "pointer", "u32"],
      result: "i32",
    },
  });

  try {
    const length = new Uint32Array(1);
    const query = kernel32.symbols.GetSystemCpuSetInformation;
    query(null, 0, length, null, 0);
    const buffer = new Uint8Array(length[0]);
    if (!query(buffer, buffer.length, length, null, 0)) {
      throw new Error("GetSystemCpuSetInformation failed");
    }

    // SYSTEM_CPU_SET_INFORMATION: Size, Type, then the CpuSet fields.
    const view = new DataView(buffer.buffer);
    const processors: Processor[] = [];
    for (let offset = 0; offset < length[0];) {
      const size = view.getUint32(offset, true);
      if (view.getUint16(offset + 12, true) === 0) {
        processors.push({
          processor: view.getUint8(offset + 14),
          core: view.getUint8(offset + 15),
          efficiency: view.getUint8(offset + 18),
        });
      }
      offset += size;
    }
    return processors;
  } finally {
    kernel32.close();
  }
}

/** Parses a Linux CPU list such as `0-7,16`. */
export function parseCpuList(list: string) {
  return list.trim().split(",").filter(Boolean).flatMap((range) => {
    const [start, end = start] = range.split("-").map(Number);
    return Array.from({ length: end - start + 1 }, (_, i) => start + i);
  });
}

/**
 * Reads the online processors Linux reports; Intel's hybrid parts list their
 * performance cores under `cpu_core` and their efficiency cores under
 * `cpu_atom`.
 */
function linuxProcessors(): Processor[] {
  const read = (path: string) => {
    try {
      return Deno.readTextFileSync(path);
    } catch {
      return undefined;
    }
  };
  const performance = new Set(
    parseCpuList(read("/sys/devices/cpu_core/cpus") ?? ""),
  );
  const online = parseCpuList(
    read("/sys/devices/system/cpu/online") ?? "",
  );

  return online.map((processor) => {
    const siblings = read(
      `/sys/devices/system/cpu/cpu${processor}/topology/thread_siblings_list`,
    );
    return {
      processor,
      // The first sibling names the core.
      core: siblings ? parseCpuList(siblings)[0] : processor,
      efficiency: performance.has(processor) ? 1 : 0,
    };
  });
}

/**
 * Pins this process to one logical processor per core of `kind`, and returns
 * the prefix that pins a command started from it. The processes a command
 * starts, the browser among them, inherit the affinity.
 */
export function pinCores(kind: CoreKind): string[] {
  switch (Deno.build.os) {
    case "windows": {
      const mask = affinityMask(chooseProcessors(windowsProcessors(), kind));
      const kernel32 = Deno.dlopen("kernel32.dll", {
        GetCurrentProcess: { parameters: [], result: "pointer" },
        SetProcessAffinityMask: {
          parameters: ["pointer", "usize"],
          result: "i32",
        },
      });
      try {
        const { GetCurrentProcess, SetProcessAffinityMask } = kernel32.symbols;
        if (!SetProcessAffinityMask(GetCurrentProcess(), BigInt(`0x${mask}`))) {
          throw new Error("SetProcessAffinityMask failed");
        }
      } finally {
        kernel32.close();
      }
      return [];
    }
    case "linux":
      return [
        "taskset",
        affinityMask(chooseProcessors(linuxProcessors(), kind)),
      ];
    default:
      throw new Error(`pinning cores is not supported on ${Deno.build.os}`);
  }
}
