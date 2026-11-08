# ADR 0002: The roaster is owned by a separate daemon process

## Status
Accepted (2026-11-08)

## Context
A roast must stay under control when parts of the system fail. A stdio MCP
server lives only as long as the AI client's session; a desktop app can crash
or be closed. If either of them owned the roaster connection, its failure would
leave the machine without control — the opposite of the safety requirements.

## Decision
A single long-running process, the daemon (`roastmcp daemon`), owns the roaster
connection, the session engine, the safety layer, the controller and the
database. The desktop app and all AI clients are clients of the daemon over a
token-protected local API (HTTP, WebSocket, MCP over Streamable HTTP). The app
starts the daemon as a bundled sidecar if it is not running. One daemon per
user, enforced by a lock file.

## Consequences
- App crashes and AI disconnects do not interrupt a roast or its safety rules.
- The MCP interface needs no separate safety logic; it is just another client.
- Two processes to package and start; the app must handle "daemon not
  reachable" as an alarm.
- The local API needs authentication (random token in a `0600` file) and must
  bind to `127.0.0.1` only.
