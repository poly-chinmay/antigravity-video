# AGENTS.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Project Overview

Ghost (Antigravity) is a professional-grade desktop video editing application built with Tauri. It features a React/TypeScript frontend (display-only) and a Rust backend that owns all state. The app includes AI-assisted editing through a hardened 7-stage LLM pipeline.

## Build & Development Commands

```bash
# Start frontend dev server
npm run dev

# Start Tauri development (frontend + Rust backend)
npm run tauri dev

# Build frontend
npm run build

# Build Tauri application
npm run tauri build

# Run all Rust tests (from src-tauri/)
cargo test

# Run performance tests in release mode
cargo test --test performance_tests --release

# Run specific test category
cargo test invariants
cargo test persistence_integration
cargo test undo_redo
```

## Architecture Overview

### Core Design Pattern: God State

The `TimelineEngine` is the sole owner of all mutable timeline state. ALL state mutations must go through `TimelineEngine::apply_action()`. This ensures:
- Deterministic replay of events
- Atomic transactions with rollback on failure
- Invariant validation on every mutation

### Component Hierarchy

```
React Frontend (display-only)
    ↓ invoke() user intents
Tauri Bridge
    ↓
AppOrchestrator (single entry point)
    ↓ coordinates
Engines (isolated, never call each other directly)
```

### Key Engines

- **TimelineEngine** (`src-tauri/src/engine/timeline_engine.rs`) - God State owner, sole mutator of timeline
- **AppOrchestrator** (`src-tauri/src/engine/orchestrator/`) - Central coordinator, atomic cross-engine effects
- **AIPipeline** (`src-tauri/src/engine/ai_pipeline.rs`) - 7-stage hardened LLM pipeline
- **PlaybackScheduler** (`src-tauri/src/engine/playback/`) - Transport + TimelineView coordination
- **WorkspaceEngine** (`src-tauri/src/engine/workspace/`) - Panel/window/focus state
- **InteractionController** (`src-tauri/src/engine/interaction/`) - Editor tools, mouse handling
- **Recovery** (`src-tauri/src/engine/recovery.rs`) - Crash-safe state reconstruction

### Frontend Components

React components in `src/components/` are display-only. They receive `STATE_UPDATE` events and send user intents via Tauri `invoke()`. No business logic or local state mutations.

## Critical Invariants

These are enforced programmatically by `InvariantValidator` on every mutation:

- Unique ClipIds (UUIDs, never reused even after deletion)
- Positive duration for all clips
- Non-negative start times
- Playhead within [0, project_duration]
- No overlapping clips on same track
- Non-empty source files for all clips

## Adding New Features

### New EditAction

1. Define in `src-tauri/src/engine/edit_action.rs`
2. Implement mutation in `TimelineEngine::execute_mutation()`
3. Add tests in `src-tauri/tests/`
4. Wire to orchestrator if needed

### New Keyboard Command

1. Define command ID in `src-tauri/src/engine/commands/command.rs`
2. Register in `CommandRegistry`
3. Add keybinding in keymap
4. Implement handler in `CommandRouter::execute_command()`

### New Tool

1. Add tool type in `src-tauri/src/engine/interaction/tools.rs`
2. Handle mouse events in `InteractionController`
3. Add preview state if needed for live feedback

## AI Pipeline Rules

The LLM never touches state directly. All LLM output must:
1. Enter through `UntrustedAIResponse` wrapper
2. Pass through 7 validation stages: Parse → Schema → Semantic → Safety → Preflight → Engine → Commit
3. Respect safety limits: max 10 deletes, max 100 actions, max 50 affected clips per request

## Persistence

Uses event sourcing with:
- Append-only event store
- Periodic snapshots (every 50 events)
- Write-ahead log for crash recovery
- Deterministic replay guarantee

## What NOT to Do

- Mutate state outside `apply_action()`
- Add business logic to React components
- Let engines call each other directly (use orchestrator)
- Trust LLM output without `UntrustedAIResponse` wrapper
- Store derived data as authoritative (rebuild indexes from source)
- Skip invariant validation after mutations
