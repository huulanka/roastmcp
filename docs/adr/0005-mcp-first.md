# ADR 0005: MCP first, in-app assistant after 1.0

## Status
Accepted (2026-11-08)

## Context
Two ways to bring AI in: expose the system through the Model Context Protocol
so any MCP client (Claude Code, Claude Desktop, others) can use it with the
user's existing subscription, or build an assistant into the app, which needs
an API key or a local model.

## Decision
- 1.0 ships an **MCP server**, served by the daemon over Streamable HTTP at
  `/mcp` (token required, `127.0.0.1` only), built with the official Rust SDK.
- `roastmcp mcp` is a **stdio bridge** for clients that only speak stdio.
- The **in-app assistant** is the first feature after 1.0 and will use the same
  tool layer and the same guardrails.

## Consequences
- No API keys or AI costs inside the app for 1.0; model-agnostic.
- Read tools are free of side effects; acting tools go through the safety layer
  and renew the AI lease.
- During a roast, the AI client runs next to the app (for example Claude Code in
  a terminal) until the in-app assistant exists.
