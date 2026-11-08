let ctx: AudioContext | null = null;

/** Browsers only allow audio after a gesture; create the context on the first one. */
export function installAudioUnlock(): void {
  const unlock = () => {
    try {
      ctx ??= new AudioContext();
      void ctx.resume();
    } catch {
      /* no audio */
    }
  };
  window.addEventListener("pointerdown", unlock, { once: true });
  window.addEventListener("keydown", unlock, { once: true });
}

export function beep(critical: boolean): void {
  if (!ctx || ctx.state !== "running") return;
  const count = critical ? 3 : 1;
  for (let i = 0; i < count; i++) {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    const start = ctx.currentTime + i * 0.28;
    osc.frequency.value = critical ? 988 : 660;
    gain.gain.setValueAtTime(0.0001, start);
    gain.gain.exponentialRampToValueAtTime(0.25, start + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.2);
    osc.connect(gain).connect(ctx.destination);
    osc.start(start);
    osc.stop(start + 0.22);
  }
}
