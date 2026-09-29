/**
 * `mediaway_rational_t`, in a leaf module.
 *
 * Split out of index.ts so `pipeline-v7.ts` (which needs it to define struct layouts that
 * embed a timebase) can import it without a cycle: index.ts imports pipeline-v7.ts, so
 * pipeline-v7.ts must not import index.ts.
 */

import koffi from "koffi";

export const MwRational = koffi.struct("MwRational", {
  num: "uint64",
  den: "uint32",
});
