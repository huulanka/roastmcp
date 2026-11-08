import { describe, expect, it } from "vitest";

import { bluetoothName, preferredPort, usefulPorts } from "./ports.ts";

const listed = [
  "/dev/cu.AKROASTER",
  "/dev/cu.debug-console",
  "/dev/tty.AKROASTER",
  "/dev/tty.debug-console",
  "/dev/tty.usbserial-1",
  "bluetooth:AKROASTER",
];

describe("ports", () => {
  it("drops the tty twin of every cu port", () => {
    expect(usefulPorts(listed)).toEqual([
      "/dev/cu.AKROASTER",
      "/dev/cu.debug-console",
      "/dev/tty.usbserial-1",
      "bluetooth:AKROASTER",
    ]);
  });

  it("names Bluetooth direct ports", () => {
    expect(bluetoothName("bluetooth:AKROASTER")).toBe("AKROASTER");
    expect(bluetoothName("/dev/cu.AKROASTER")).toBeNull();
  });

  it("prefers the last port, then the roaster's serial port", () => {
    const ports = usefulPorts(listed);
    expect(preferredPort(ports, "bluetooth:AKROASTER")).toBe("bluetooth:AKROASTER");
    expect(preferredPort(ports, "/dev/cu.gone")).toBe("/dev/cu.AKROASTER");
    expect(preferredPort(ports, null)).toBe("/dev/cu.AKROASTER");
    expect(preferredPort([], null)).toBeUndefined();
  });
});
