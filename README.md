# RoastMCP

Open-source coffee roasting software for home and small-batch roasters.
Live roasting, a roast archive, green coffee inventory, profile development
and cupping in one desktop app — with an AI interface (the
[Model Context Protocol](https://modelcontextprotocol.io)) that lets an
assistant such as Claude analyse your roasts, coach you through the next one,
and, under strict guardrails and with you standing next to the roaster,
drive a roast.

> **Status: pre-alpha.** Live roasting with the simulator and the Skywalker
> Cubean+ works in the browser, with the safety layer, the archive and MCP.
> Everything else is being built milestone by milestone
> ([roadmap](docs/roadmap.md)). Hardware use is experimental: stay at the
> roaster and read [docs/first-roast.md](docs/first-roast.md) first.

## Safety first

A coffee roaster is a heat source that can start a fire. RoastMCP is built
around one rule: **a person supervises every roast and can always take over.**

- Anything you do by hand wins over the AI, immediately.
- The AI can only act within limits (step size, rate, range) and only while
  you keep confirming that you are present.
- The emergency stop does not depend on the AI or the user interface.
- The software adds guardrails; it does not replace you. Never leave a roast
  unattended. See [docs/safety.md](docs/safety.md).

RoastMCP is provided under the Apache License 2.0, **without warranty of any
kind**. You use it at your own risk.

## Try it

You need Rust (stable) and Node.js 22 or newer.

```sh
cd app && npm ci && npm run build && cd ..        # the web UI
cargo build --release -p roastmcp
./target/release/roastmcp daemon --simulator       # or: --serial /dev/cu.AKROASTER
```

The daemon prints a link (`http://127.0.0.1:7878/#token=…`); open it in a
browser. Or build the desktop app, which starts the daemon itself, adds a
system-wide emergency stop (⌘⇧⎋ on macOS, Ctrl+Alt+Esc elsewhere) and asks
before quitting during a roast:

```sh
cd app && npx tauri build --bundles app    # app/src-tauri/target/release/bundle/macos/RoastMCP.app
```

Connect the roaster or the simulator from the app. Roasts are saved as `.roast.json` and Artisan `.alog` in the data
directory (`~/Library/Application Support/com.andreasbauer.roastmcp/roasts`
on macOS).

To let Claude read and, under guardrails, control the roast, register the MCP
server (it talks to the running daemon):

```sh
claude mcp add roastmcp -- "$PWD/target/release/roastmcp" mcp
```

For Claude Desktop, add `{"command": "/path/to/roastmcp", "args": ["mcp"]}`
under `mcpServers`. The desktop app carries the same binary at
`RoastMCP.app/Contents/MacOS/roastmcp`. Before the AI may control anything, confirm in the app
that you are at the roaster.

## What 1.0 will do

- **Roast live** — bean temperature and rate of rise, events (charge, turning
  point, dry end, first crack, drop), controls for burner, air and drum,
  configurable keys and alarms, sound cues.
- **Develop profiles** — reference curves with live deviation and projection,
  profile versions, experiments (hypothesis, changed variable, outcome),
  comparisons and consistency statistics.
- **Hear first crack** — first-crack detection from a microphone, shown on
  screen as well.
- **Digital twin** — a thermal model of your roaster, calibrated from your own
  logs, for predictions, what-if tests and training — with a 3D view of the
  drum.
- **Keep everything** — roast archive (imports Artisan `.alog`), green coffee
  inventory, cupping, roast degree and ambient conditions, rest-time reminders,
  maintenance reminders.
- **Let AI help** — an MCP server that Claude Code, Claude Desktop or any other
  MCP client can use to read, coach and — with guardrails — control a roast.
- **Guide beginners** — setup wizard, guided roasting with starter profiles,
  a glossary, a pre-roast safety checklist.

Your data stays on your machine: a local database plus one open file per roast,
backed up automatically to a location you choose (for example a NAS).
No telemetry.

## Roasters

RoastMCP talks to roasters through drivers. The first driver covers the
**TC4 family** — roasters and controllers that speak the serial protocol made
popular by the TC4 shield and Artisan — configured through machine profiles
that can be imported from Artisan `.aset` files. The first validated machine
is the **Skywalker Cubean+** over Bluetooth. A built-in simulator lets you try
everything without a roaster.

macOS is the officially supported platform. Windows and Linux builds are
provided as a preview until they have been confirmed on real hardware.

## Documentation

- [Product scope](docs/product.md) · [Architecture](docs/architecture.md) ·
  [Safety](docs/safety.md) · [Roadmap](docs/roadmap.md)
- [Architecture Decision Records](docs/adr/)
- [Research: Artisan and the Cubean+](docs/research/artisan-and-cubean-plus.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

## License

[Apache License 2.0](LICENSE). RoastMCP is not affiliated with the Artisan
project and contains no Artisan code; it reads and writes Artisan's `.alog`
format for compatibility.
