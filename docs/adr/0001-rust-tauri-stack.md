# ADR 0001: Rust and Tauri v2, with a React + TypeScript interface

## Status
Accepted (2026-11-08)

## Context
RoastMCP is a desktop application that talks to hardware over serial and
Bluetooth, runs a real-time sampling loop with safety rules, stores years of
data, and needs a polished interface including live charts and a 3D view. It
should run on macOS, Windows and Linux and be installable through Homebrew.
Python (the language of Artisan) was considered: its MCP SDK is the reference
implementation and `.alog` files are Python literals. Rust with Tauri has
already worked well for the author on the Mac (Hippocampus).

## Decision
- Core, daemon, drivers and CLI in **Rust** (stable toolchain), async on
  **tokio**, HTTP with **axum**, SQLite with **sqlx**, MCP with the official
  Rust SDK **rmcp**, serial with **serialport**.
- Desktop app on **Tauri v2** with **Vite + React + TypeScript**, Tailwind CSS
  and shadcn/ui, uPlot for curves, React Three Fiber for 3D, i18next.

## Consequences
- One language for everything safety-relevant; strong typing and no garbage
  collector pauses in the sampling loop.
- Small, native installers on all three platforms.
- `.alog` needs a small Python-literal parser of our own (`roast-alog`).
- Contributors need Rust or TypeScript, not Python; machine profiles (TOML)
  need neither.
