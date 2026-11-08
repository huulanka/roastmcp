/**
 * Ports as the daemon lists them: serial ports (`/dev/cu.AKROASTER`) and, on a
 * Mac, paired Bluetooth serial devices as `bluetooth:AKROASTER` (ADR 0013).
 */

export const BLUETOOTH_PREFIX = "bluetooth:";

/** The Bluetooth device name of a `bluetooth:` port, else null. */
export function bluetoothName(port: string): string | null {
  return port.startsWith(BLUETOOTH_PREFIX) ? port.slice(BLUETOOTH_PREFIX.length) : null;
}

/**
 * The ports worth offering: macOS lists every serial device twice, as
 * `/dev/cu.X` (for programs that call out) and `/dev/tty.X` (which waits for a
 * carrier signal and hangs); only the `cu` one is useful.
 */
export function usefulPorts(ports: readonly string[]): string[] {
  const set = new Set(ports);
  return ports.filter(
    (p) => !(p.startsWith("/dev/tty.") && set.has(`/dev/cu.${p.slice("/dev/tty.".length)}`)),
  );
}

/**
 * The port to preselect: the last one used if it is still there, else the
 * roaster's serial port, else the first.
 */
export function preferredPort(ports: readonly string[], last: string | null): string | undefined {
  if (last && ports.includes(last)) return last;
  return ports.find((p) => !bluetoothName(p) && p.toUpperCase().includes("AKROASTER")) ?? ports[0];
}

const LAST_PORT_KEY = "roastmcp.lastPort";

/** The last port connected to, remembered in this browser (may be unavailable). */
export function lastPort(): string | null {
  try {
    return localStorage.getItem(LAST_PORT_KEY);
  } catch {
    return null;
  }
}

export function rememberPort(port: string): void {
  try {
    localStorage.setItem(LAST_PORT_KEY, port);
  } catch {
    /* private window or blocked storage: nothing to remember */
  }
}
