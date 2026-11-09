/** Fallback rate of rise (°C/min) for samples that carry none: slope over a trailing window (20 s, as the daemon and Artisan). */
export function computeRor(
  ts: readonly number[],
  bts: readonly (number | null)[],
  windowS = 20,
): (number | null)[] {
  const out: (number | null)[] = [];
  let j = 0;
  for (let i = 0; i < ts.length; i++) {
    while (j < i && ts[i] - ts[j] > windowS) j++;
    const a = bts[j];
    const b = bts[i];
    const span = ts[i] - ts[j];
    out.push(a === null || b === null || span < 10 ? null : ((b - a) / span) * 60);
  }
  return out;
}
