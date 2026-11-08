# Research: Artisan and the Skywalker Cubean+

Findings from November 2026 that shaped the design. Protocol facts below were
established by reading the vendor's Artisan settings file, recorded roast logs
and the public Artisan source to understand what goes over the wire. RoastMCP
contains no Artisan code ([ADR 0007](../adr/0007-license-and-clean-room.md)).

## How the Cubean+ is connected

- There is no Cubean-specific driver in Artisan. The vendor (Foshan Akimita,
  hence the Bluetooth name `AKROASTER`) ships an Artisan settings file that uses
  Artisan's generic **TC4** device.
- The link is **Bluetooth Classic, Serial Port Profile**. macOS creates
  `/dev/cu.AKROASTER`; Windows shows a COM port; Linux an `rfcomm` device.
  115200 baud, 8N1.
- Artisan opens the port exclusively, so two programs cannot share it.

## The protocol (TC4 dialect)

Line-based ASCII, `\n`-terminated.

| Direction | Message | Meaning |
|---|---|---|
| → | `CHAN;2100` | Channel mapping (ET on 2, BT on 1). Reply starts with `#` |
| → | `UNITS;C` | Units. Reply starts with `#` |
| → | `FILT;95,95,95,95` | Filter levels. Reply starts with `#` |
| → | `READ` | Request a sample |
| ← | `amb,ET,BT,burner,air,SV` | Ambient, environment temp, bean temp, burner %, air %, setpoint |
| → | `OT1,<0-100>` | Burner |
| → | `OT2,<0-100>` | Air (fan) |
| → | `OT3,<80-100>` | Drum speed (100 % ≈ 68 rpm, 80 % ≈ 55 rpm) |
| → | `OT4,<0-100>` | Filter fan |
| → | `CLDN,ON` / `CLDN,OF` | Bean cooling on/off (the vendor file sends `OF`; Artisan's Kaleido preset uses `OFF` — to verify) |

Commands are fire-and-forget; Artisan discards any reply before the next
`READ`. The Cubean+ has no environment probe (ET reads a constant, tared to 0
in the vendor file). It reports burner and air back in `READ`, which makes the
actual state observable; reported burner values sometimes differ from the set
value (for example 21.7 or 28.3 %), so the firmware ramps or quantises — to be
characterised in M1. The setpoint field is constant (unused).

The command set resembles Artisan's "Kaleido Legacy" machine preset
(`OT1`, `OT2`, `CLDN`), which suggests a shared firmware lineage.

## The vendor's Artisan setup, decoded

- Sampling every 1.5 s.
- Extra device "+ArduinoTC4 56" shows the reported burner and air; Artisan's
  *quantifiers* turn changes of those values into events.
- Extra device "+PID SV/DUTY %" (labelled "Heater") only shows values while
  Artisan's software PID is on — not a fault.
- Software PID (Kp 15, Ki 0.07, Kd 0.06) on bean temperature driving the burner,
  started at CHARGE.

## Artisan's external interfaces (5.0), for reference

Artisan has no inbound control API for burner, air or drum. From outside, only
CHARGE, DROP and special events can be pushed (WebSocket device), measurement
channels can be fed in (WebSocket, MQTT), and profiles can be opened via
`artisan://` URLs. This is why RoastMCP owns the roaster connection itself
instead of bridging to Artisan.

## Lessons from the Skywalker community

- On older Skywalker models the main board shuts down if the controller stops
  talking for about a second; community firmware stops after 10 s without a
  command.
- Their warning: switching everything off during a roast is dangerous because
  hot beans without airflow or drum movement can ignite. Hence RoastMCP's safe
  state: burner off, air up, drum running.

## Sources

- Artisan source, v5.0.0: <https://github.com/artisan-roaster-scope/artisan/tree/v5.0.0>
- Artisan 5.0.0 release (AGPLv3): <https://github.com/artisan-roaster-scope/artisan/releases/tag/v5.0.0>
- Skywalker Bluetooth pairing ("AKROASTER"): <https://www.topcoffee.net/media/coffee/datasheets/SKYWALKER-Delta-Coffee-Roaster_Manual_topcoffee.pdf>
- jmoore52/SkywalkerRoaster: <https://github.com/jmoore52/SkywalkerRoaster>
- juncaffe/SkywalkerRoasterLab: <https://github.com/juncaffe/SkywalkerRoasterLab>
- TC4 emulator: <https://github.com/FilePhil/TC4-Emulator>
- RoastPilot (MCP server for the Hottop): <https://github.com/syamaner/coffee-roaster-mcp>
- HiBean connection methods: <https://hibean-gitbook.pages.dev/hardware/connection-methods>
