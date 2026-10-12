# Antigravity Engineering Overview

> The canonical technical reference for senior engineers joining the Antigravity project.

---

## Table of Contents

1. [Project Overview](#1-project-overview)
2. [System Architecture Overview](#2-system-architecture-overview)
3. [Core Design Principles & Invariants](#3-core-design-principles--invariants)
4. [Engine Breakdown](#4-engine-breakdown)
5. [Data Flow & Event Flow](#5-data-flow--event-flow)
6. [Persistence & Recovery Model](#6-persistence--recovery-model)
7. [Testing & Reliability Strategy](#7-testing--reliability-strategy)
8. [How to Work in This Codebase](#8-how-to-work-in-this-codebase)
9. [Roadmap Snapshot](#9-roadmap-snapshot)

---

## 1. Project Overview

### 1.1 Vision & Goals

Antigravity is a professional-grade desktop video editing application with native AI assistance. The core mission is to create a video editor that is:

- **Deterministic**: Given the same inputs, produce identical outputs every time
- **Recoverable**: Never lose user work, even on crash
- **AI-Augmented**: LLM-driven editing that operates through safe, validated pipelines
- **Local-First**: No cloud dependency for core functionality; all processing happens on-device

### 1.2 Target Users

- Professional video editors requiring precise, reliable tools
- Content creators seeking AI-assisted workflow acceleration
- Developers building on the Antigravity engine

### 1.3 Product Philosophy

| Principle | Implementation |
|-----------|----------------|
| **Privacy-First** | All data stays local; no telemetry without consent |
| **Local-First** | Full functionality offline; cloud features are additive |
| **Deterministic** | Immutable ClipIds, single mutation path, replay guarantees |
| **AI-Assisted** | LLM never touches state directly; 7-stage hardened pipeline |

### 1.4 Non-Goals

- Web-based/cloud-only editor
- Mobile application (desktop-first)
- Real-time collaboration (v1 scope)
- Streaming/live production features

---

## 2. System Architecture Overview

### 2.1 High-Level System Diagram

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                           REACT FRONTEND ("Dumb Display")                   │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │   App.tsx   │  │   Timeline  │  │ VideoPlayer │  │  MissionControl     │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────────────┘ │
│                                                                              │
│  ► Receives STATE_UPDATE events only                                         │
│  ► No business logic, no local state mutations                               │
│  ► Sends user intents via Tauri invoke()                                     │
└──────────────────────────────────┬──────────────────────────────────────────┘
                                   │
                           ┌───────▼───────┐
                           │  TAURI BRIDGE │
                           └───────┬───────┘
                                   │
┌──────────────────────────────────▼──────────────────────────────────────────┐
│                      APP ORCHESTRATOR (Single Authority)                     │
│                                                                              │
│  ► Central coordinator for ALL engines                                       │
│  ► Single entry point from Tauri                                             │
│  ► Atomic cross-engine effects                                               │
│  ► Engines NEVER call each other directly                                    │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                              ENGINES                                  │   │
│  │                                                                       │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐   │   │
│  │  │  WORKSPACE   │  │   TIMELINE   │  │    PLAYBACK SCHEDULER    │   │   │
│  │  │   ENGINE     │  │    ENGINE    │  │                          │   │   │
│  │  │              │  │              │  │  ┌────────┐ ┌──────────┐ │   │   │
│  │  │ Panel state  │  │ God State    │  │  │Transport│ │TimelineView│ │   │
│  │  │ Window state │  │ Clips/Tracks │  │  └────────┘ └──────────┘ │   │   │
│  │  │ Focus mgmt   │  │ Undo/Redo    │  │                          │   │   │
│  │  └──────────────┘  └──────────────┘  └──────────────────────────┘   │   │
│  │                                                                       │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐   │   │
│  │  │    RENDER    │  │    AUDIO     │  │     INTERACTION          │   │   │
│  │  │ ORCHESTRATOR │  │    ENGINE    │  │     CONTROLLER           │   │   │
│  │  │              │  │              │  │                          │   │   │
│  │  │ Frame cache  │  │ Audio clock  │  │ Tools (select, move,     │   │   │
│  │  │ Frame queue  │  │ A/V sync     │  │ trim, razor, playhead)   │   │   │
│  │  │ Scheduler    │  │ Device mgmt  │  │ Snapping, preview        │   │   │
│  │  └──────────────┘  └──────────────┘  └──────────────────────────┘   │   │
│  │                                                                       │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐   │   │
│  │  │   COMMAND    │  │   UI BRIDGE  │  │      AI PIPELINE         │   │   │
│  │  │    SYSTEM    │  │              │  │                          │   │   │
│  │  │              │  │ View models  │  │ Parse → Schema →         │   │   │
│  │  │ Registry     │  │ Event emit   │  │ Semantic → Safety →      │   │   │
│  │  │ Keymap       │  │ Throttling   │  │ Preflight → Engine →     │   │   │
│  │  │ Router       │  │              │  │ Commit                   │   │   │
│  │  └──────────────┘  └──────────────┘  └──────────────────────────┘   │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                          PERSISTENCE LAYER                            │   │
│  │                                                                       │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐ │   │
│  │  │ Event Store │  │  Snapshots  │  │     WAL     │  │  Recovery   │ │   │
│  │  │             │  │             │  │             │  │   Engine    │ │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘ │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Component Responsibilities

| Component | Primary Responsibility |
|-----------|----------------------|
| **React Frontend** | Display-only layer; receives `STATE_UPDATE` events, sends user intents |
| **Tauri Bridge** | IPC layer between React and Rust; exposes commands (`invoke`) |
| **AppOrchestrator** | Central coordinator; single entry point; atomic cross-engine effects |
| **TimelineEngine** | God State owner; sole mutator of timeline state via `apply_action()` |
| **WorkspaceEngine** | Owns workspace state (panels, windows, focus); sole mutator via `apply_command()` |
| **PlaybackScheduler** | Coordinates Transport + TimelineView for playback |
| **RenderOrchestrator** | Frame pipeline coordinator; cache, queue, scheduling |
| **AudioEngine** | Audio clock, A/V synchronization, device management |
| **InteractionController** | Editor tools, mouse/keyboard handling, preview state |
| **CommandSystem** | Registry, keymap, routing for keyboard shortcuts |
| **UIBridge** | Engine-to-UI one-way bridge; emits STATE_UPDATE events |
| **AIPipeline** | Hardened 7-stage LLM processing pipeline |
| **Persistence** | Event sourcing, snapshots, WAL, crash recovery |

---

## 3. Core Design Principles & Invariants

### 3.1 Constitutional Invariants

These rules are non-negotiable and enforced programmatically by `InvariantValidator`:

| Invariant | Rule | Why It Exists |
|-----------|------|---------------|
| **Unique ClipIds** | `∀ clip₁, clip₂: clip₁.id ≠ clip₂.id` | Enables undo, AI tracking, export determinism |
| **Positive Duration** | `∀ clip: clip.duration > 0` | Zero-duration clips are undefined behavior |
| **Non-negative Start** | `∀ clip: clip.start >= 0` | No clip before timeline origin |
| **Playhead Bounds** | `playhead ∈ [0, project_duration]` | Playhead must be within valid range |
| **No Overlaps** | No two clips on same track overlap | Single-track V1 model; prevents undefined rendering |
| **Project Duration** | `project_duration >= max(clip.end())` | Timeline length always covers content |
| **Single Frame Rate** | `timeline_frame_rate > 0` | One authoritative frame rate per project |
| **Non-empty Sources** | `∀ clip: clip.source_file != ""` | Every clip must reference a valid file |

### 3.2 Architectural Invariants

| Invariant | Description |
|-----------|-------------|
| **Single Mutation Path** | ALL state mutations go through `TimelineEngine::apply_action()` |
| **God State Pattern** | `TimelineEngine` is the sole owner of mutable timeline state |
| **Engine Isolation** | Engines NEVER call each other directly; always through orchestrator |
| **No UI Business Logic** | React frontend is display-only; no local state mutations |
| **Snapshots are Immutable** | `snapshot()` returns a clone; caller cannot mutate engine state |
| **Index is Derived** | TimelineIndex is rebuilt from clips; never authoritative |
| **Deterministic Replay** | Same events replayed produce identical state |
| **Atomic Transactions** | Mutations are all-or-nothing; rollback on validation failure |

### 3.3 Time Precision Policy

```rust
const EPSILON: f64 = 0.001; // 1ms tolerance for floating-point comparisons
```

- All time values use `f64` seconds internally
- Frame-accurate seeking: `frame = floor(time * fps)`
- Comparisons use millisecond tolerance

---

## 4. Engine Breakdown

### 4.1 TimelineEngine

**Purpose**: The God State owner. Sole mutator of all timeline state.

**Location**: `src-tauri/src/engine/timeline_engine.rs`

**Public API Summary**:
```rust
// Read access (concurrent, no lock contention)
fn snapshot(&self) -> TimelineState    // Clone of current state
fn version(&self) -> u64               // Current version number
fn clip_count(&self) -> usize          // Number of clips
fn can_undo(&self) -> bool
fn can_redo(&self) -> bool

// Write access (exclusive lock, single mutation path)
fn apply_action(&self, action: EditAction) -> Result<ApplyResult, EngineError>
fn undo(&self) -> Result<UndoResult, EngineError>
fn redo(&self) -> Result<UndoResult, EngineError>
```

**Internal Model**:
- `state: RwLock<TimelineState>` - The authoritative state
- `undo_manager: RwLock<UndoManager>` - Undo/redo stack
- `event_store: RwLock<EventStore>` - Append-only event log
- `version: AtomicU64` - Monotonic version counter

**Mutation Rules**:
1. All mutations via `apply_action()` only
2. Mutation acquires exclusive write lock
3. State cloned before mutation (for rollback)
4. Invariants validated after mutation
5. On validation failure: rollback and return error
6. On success: commit event, update version, push undo entry

**Threading Model**: `RwLock` for concurrent reads, exclusive writes

**Invariants Enforced**: All constitutional invariants checked on every mutation

---

### 4.2 PlaybackScheduler

**Purpose**: Coordinates Transport with TimelineView for complete playback system.

**Location**: `src-tauri/src/engine/playback/scheduler.rs`

**Public API Summary**:
```rust
fn new(config: SchedulerConfig, duration: MediaTime) -> Self
fn get_frame(&mut self, index: &TimelineIndex, state: &TimelineState) -> FrameInfo
fn execute(&mut self, cmd: TransportCommand)
fn play(&mut self) / fn pause(&mut self) / fn stop(&mut self)
fn seek(&mut self, position: MediaTime)
fn set_rate(&mut self, rate: PlaybackRate)
fn position(&self) -> MediaTime
fn state(&self) -> TransportState
fn is_playing(&self) -> bool
```

**Internal Model**:
- `transport: Transport` - Playback state machine
- `view: TimelineView` - Visible clip calculation
- `config: SchedulerConfig` - Frame rate, lookahead settings

**Threading Model**: Interior mutability; typically wrapped in `Arc<RwLock<_>>`

---

### 4.3 RenderOrchestrator

**Purpose**: Pipeline coordinator for frame rendering.

**Location**: `src-tauri/src/engine/render/render_orchestrator.rs`

**Public API Summary**:
```rust
fn new(config: OrchestratorConfig) -> Self
fn submit(&mut self, cmd: RenderCommand) -> bool  // Submit frame for rendering
fn process(&mut self)                              // Process pending commands
fn poll_result(&mut self) -> Option<RenderResult>  // Get completed results
fn has_work(&self) -> bool
fn clear(&mut self)
fn cache_stats(&self) -> &CacheStats
```

**Internal Model**:
- `pending: VecDeque<RenderCommand>` - Queue of commands awaiting dispatch
- `in_flight: HashMap<FrameId, RenderCommand>` - Currently rendering
- `completed: VecDeque<RenderResult>` - Finished results
- `cache: FrameCache` - LRU frame cache

**Mutation Rules**: Submit → Process → Complete → Poll

---

### 4.4 AudioEngine (AVSync)

**Purpose**: Audio/Video synchronization controller. Audio is master clock.

**Location**: `src-tauri/src/engine/audio/av_sync.rs`

**Public API Summary**:
```rust
fn new(audio_clock: AudioClock, config: SyncConfig) -> Self
fn audio_time(&self) -> MediaTime           // Master time
fn video_target_time(&self) -> MediaTime    // Where video should be
fn status_for(&self, video_time: MediaTime) -> SyncStatus
fn should_skip(&self, video_time: MediaTime) -> bool  // Video behind
fn should_wait(&self, video_time: MediaTime) -> bool  // Video ahead
fn start(&mut self) / fn pause(&mut self) / fn stop(&mut self)
fn seek(&mut self, position: MediaTime)
```

**Synchronization Status**:
```rust
enum SyncStatus {
    InSync,       // Within tolerance
    VideoAhead,   // Need to wait
    VideoBehind,  // Need to catch up
    Seeking,      // Sync suspended
    Paused,       // Sync suspended
}
```

**Invariants**: Video never leads audio by more than 1 frame

---

### 4.5 WorkspaceEngine

**Purpose**: Sole owner of mutable workspace state (panels, windows, focus).

**Location**: `src-tauri/src/engine/workspace/workspace_engine_v2.rs`

**Public API Summary**:
```rust
fn new() -> Self
fn snapshot(&self) -> WorkspaceState          // Returns CLONE
fn apply_command(&self, cmd: WorkspaceCommand) -> WorkspaceResult<()>
fn apply_commands(&self, commands: Vec<WorkspaceCommand>) -> WorkspaceResult<()>
fn checksum(&self) -> String
fn is_modified_since(&self, timestamp: u64) -> bool
```

**Commands**: Open/close project, switch active project, show/hide panel, move panel, change focus, set window state, etc.

**Threading Model**: `RwLock<WorkspaceState>`

---

### 4.6 InteractionController

**Purpose**: Main coordinator for editor interactions (tools, mouse, preview).

**Location**: `src-tauri/src/engine/interaction/interaction_controller.rs`

**Public API Summary**:
```rust
fn new(config: ControllerConfig) -> Self
fn state(&self) -> &InteractionState
fn current_tool(&self) -> ToolType
fn set_tool(&mut self, tool: ToolType)
fn preview(&self) -> Option<&PreviewState>
fn selected_clips(&self) -> &[ClipId]

// Mouse event handlers
fn on_mouse_down(&mut self, input: MouseInput, hit_clip: Option<&Clip>, timeline: &TimelineState) -> InteractionResult
fn on_mouse_move(&mut self, input: MouseInput, timeline: &TimelineState, playhead: MediaTime) -> InteractionResult
fn on_mouse_up(&mut self) -> InteractionResult
fn cancel(&mut self) -> InteractionResult
```

**Tool Types**: Select, Move, Trim (left/right edge), Razor, Playhead

**Interaction Flow**:
1. Mouse down → determine target (clip, edge, playhead)
2. Mouse move → update preview state (no mutation)
3. Mouse up → commit (generate EditAction) or cancel

---

### 4.7 Command System

**Purpose**: Keyboard shortcut handling, command registry, and routing.

**Location**: `src-tauri/src/engine/commands/`

**Components**:
- `CommandRegistry` - Maps CommandId to CommandDescriptor
- `Keymap` - Maps KeyBinding to CommandId
- `CommandRouter` - Dispatches commands, applies effects

**Public API Summary**:
```rust
// Router
fn dispatch_key(&self, key: &KeyBinding, snapshot: &CommandSnapshot, ...) -> RouterResult
fn dispatch_command(&self, cmd_id: &CommandId, snapshot: &CommandSnapshot, ...) -> RouterResult

// Registry
fn register(&mut self, descriptor: CommandDescriptor)
fn get(&self, id: &CommandId) -> Option<&CommandDescriptor>
fn all_commands(&self) -> impl Iterator<Item = &CommandDescriptor>

// Keymap  
fn bind(&mut self, binding: KeyBinding, cmd_id: CommandId)
fn lookup(&self, binding: &KeyBinding) -> Option<&CommandId>
```

---

### 4.8 UI Bridge

**Purpose**: One-way bridge from engine to UI. Emits `STATE_UPDATE` events.

**Location**: `src-tauri/src/engine/ui/bridge.rs`

**Public API Summary**:
```rust
fn new(sender: UIEventSender, config: BridgeConfig) -> Self
fn on_state_changed(&mut self, state: &TimelineState, playback: &PlaybackScheduler, reason: UpdateReason)
fn on_mutation_committed(&mut self, state: &TimelineState, playback: &PlaybackScheduler, reason: UpdateReason)
fn on_playhead_tick(&mut self, playback: &PlaybackScheduler)
fn get_view_model(&self, state: &TimelineState, playback: &PlaybackScheduler) -> TimelineViewModel
```

**One-Way Flow**:
```text
Engine (Rust) → UIBridge → STATE_UPDATE event → React Frontend
```

- React NEVER sends state back; only sends user intents
- UIBridge throttles playhead updates to avoid flooding

---

### 4.9 AI Pipeline (AIPipeline)

**Purpose**: Hardened control surface for LLM-driven edits.

**Location**: `src-tauri/src/engine/ai_pipeline.rs`

**Design Invariants**:
1. LLM NEVER touches state directly
2. ALL LLM output enters through `UntrustedAIResponse`
3. 7-stage pipeline with fail-fast at each stage

**Pipeline Stages**:
```text
Stage 1: PARSE        - Extract JSON from raw LLM output
Stage 2: SCHEMA       - Validate against AIEditPlan schema
Stage 3: SEMANTIC     - Verify referenced clips exist
Stage 4: SAFETY       - Apply rate limits and safety rules
Stage 5: PREFLIGHT    - Simulate actions, detect conflicts
Stage 6: ENGINE       - Apply validated actions to engine
Stage 7: COMMIT       - Persist to event store
```

**Safety Rules** (`SafetyRule`):
- `TooManyDeletes` - Max 10 deletes per request
- `TooManyAffectedClips` - Max 50 clips affected
- `TooManyActions` - Max 100 actions per request
- `NegativePosition` / `InvalidDuration` - Value validation
- `PathTraversal` / `AbsolutePath` - File path sanitization
- `NonExistentClipId` - Reference validation

**Public API**:
```rust
fn process(&mut self, response: UntrustedAIResponse) -> AIResult
```

**Result States**:
```rust
enum AIResult {
    Accepted { actions_applied: usize, thought_process: Option<String> },
    Rejected { failure: AIFailure, message: String },
}
```

---

## 5. Data Flow & Event Flow

### 5.1 User Input → Engine Mutation

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ USER CLICKS "DELETE CLIP"                                                │
└─────────────────────┬────────────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ React: invoke("execute_edit", { action: { type: "DELETE", clip_id } })   │
└─────────────────────┬────────────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ Tauri Command: Deserialize → AppOrchestrator::apply()                    │
└─────────────────────┬────────────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ AppOrchestrator::apply_timeline_action(EditAction::Delete { clip_id })   │
│   1. Acquire write lock on TimelineEngine                                │
│   2. Clone state (for rollback)                                          │
│   3. Execute mutation                                                    │
│   4. Validate invariants                                                 │
│   5. On success: commit event, push undo, update version                 │
│   6. On failure: rollback, return error                                  │
└─────────────────────┬────────────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ UIBridge::on_mutation_committed() → emit("STATE_UPDATE", new_state)      │
└─────────────────────┬────────────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ React: listen("STATE_UPDATE") → setTimelineState(payload)                │
└──────────────────────────────────────────────────────────────────────────┘
```

### 5.2 AI Edit Flow (7-Stage Pipeline)

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ USER: "Remove all clips shorter than 2 seconds"                          │
└─────────────────────┬────────────────────────────────────────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 1: SEND TO LLM                   │
│ Build prompt with timeline context     │
│ Send to local Ollama (llama3.2)        │
└─────────────────────┬──────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 2: WRAP IN UntrustedAIResponse   │
│ All LLM output MUST enter here         │
│ No parsing outside this boundary       │
└─────────────────────┬──────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 3: PARSE JSON                    │
│ Extract AIEditPlan from raw output     │
│ ✗ Fail: AIFailure::ParseError          │
└─────────────────────┬──────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 4: VALIDATE SCHEMA               │
│ Check structure, required fields       │
│ Reject unknown fields                  │
│ ✗ Fail: AIFailure::SchemaViolation     │
└─────────────────────┬──────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 5: SEMANTIC VALIDATION           │
│ Verify all clip_ids exist              │
│ Verify track_ids exist                 │
│ ✗ Fail: NonExistentClipId              │
└─────────────────────┬──────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 6: SAFETY CHECK                  │
│ TooManyDeletes (max 10)                │
│ TooManyActions (max 100)               │
│ TooManyAffectedClips (max 50)          │
│ PathTraversal, AbsolutePath            │
│ ✗ Fail: AIFailure::SafetyViolation     │
└─────────────────────┬──────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 7: PREFLIGHT SIMULATION          │
│ Clone state, apply actions             │
│ Validate invariants on simulated state │
│ Detect conflicts BEFORE real mutation  │
│ ✗ Fail: AIFailure::PreflightFailed     │
└─────────────────────┬──────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 8: ENGINE APPLICATION            │
│ Apply actions via TimelineEngine       │
│ Single transaction, all-or-nothing     │
│ ✗ Fail: AIFailure::EngineRejected      │
└─────────────────────┬──────────────────┘
                      │
                      ▼
┌────────────────────────────────────────┐
│ Stage 9: COMMIT & EMIT                 │
│ Persist to event store                 │
│ Emit STATE_UPDATE to frontend          │
│ Return AIResult::Accepted              │
└────────────────────────────────────────┘
```

### 5.3 Playback & Render Scheduling

```text
┌───────────────────────────────────────────────────────────────────────────┐
│                        PLAYBACK LOOP (60fps)                              │
├───────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│  1. PlaybackScheduler.get_frame(index, state)                             │
│     ├── Transport.tick() → update position                                │
│     └── TimelineView.get_visible_clips(position, state)                   │
│                                                                           │
│  2. AVSync.video_target_time()                                            │
│     ├── audio_time = AudioClock.current_time()                            │
│     └── Return target video time (follows audio)                          │
│                                                                           │
│  3. AVSync.status_for(video_time)                                         │
│     ├── InSync → proceed normally                                         │
│     ├── VideoAhead → wait before display                                  │
│     └── VideoBehind → skip frame to catch up                              │
│                                                                           │
│  4. RenderOrchestrator.submit(RenderCommand)                              │
│     ├── Check cache (O(1) lookup)                                         │
│     ├── Cache hit → return immediately                                    │
│     └── Cache miss → queue for rendering                                  │
│                                                                           │
│  5. RenderOrchestrator.poll_result()                                      │
│     └── Return completed frame for display                                │
│                                                                           │
│  6. UIBridge.on_playhead_tick(scheduler)                                  │
│     └── Emit PlayheadMoved event (throttled)                              │
│                                                                           │
└───────────────────────────────────────────────────────────────────────────┘
```

### 5.4 Snapshot Propagation to UI

```text
Mutation committed → UIBridge.on_mutation_committed()
                          │
                          ▼
                   Build TimelineViewModel:
                   - clips: Vec<ClipView>
                   - playhead_time: f64
                   - duration: f64
                   - version: u64
                          │
                          ▼
                   emit("STATE_UPDATE", view_model)
                          │
                          ▼
           React: setTimelineState(event.payload)
```

### 5.5 Crash Recovery Flow

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                      APPLICATION STARTUP                                 │
└─────────────────────┬───────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ RecoveryEngine::needs_recovery()                                        │
│ Check for uncommitted events in WAL                                     │
└─────────────────────┬───────────────────────────────────────────────────┘
                      │
          ┌───────────┴───────────┐
          │                       │
    [No recovery needed]    [Recovery needed]
          │                       │
          ▼                       ▼
┌───────────────────┐  ┌─────────────────────────────────────────────────┐
│ Load normally     │  │ RecoveryEngine::recover()                       │
└───────────────────┘  │ 1. Load newest valid snapshot                   │
                       │ 2. Load committed events after snapshot         │
                       │ 3. Replay events in order                       │
                       │ 4. Validate invariants                          │
                       │ 5. Return RecoveryResult                        │
                       └─────────────────────┬───────────────────────────┘
                                             │
                       ┌─────────────────────┴─────────────────────┐
                       │                                           │
                 [Success]                                    [Failure]
                       │                                           │
                       ▼                                           ▼
            ┌─────────────────────┐               ┌───────────────────────────┐
            │ Use recovered state │               │ RecoveryError             │
            │ Show recovery msg   │               │ - AllSnapshotsCorrupted   │
            └─────────────────────┘               │ - ReplayFailed            │
                                                  │ - Corrupted               │
                                                  └───────────────────────────┘
```

### 5.6 Undo/Redo Flow

```text
User: Cmd+Z (Undo)
       │
       ▼
TimelineEngine::undo()
       │
       ├── Check can_undo() → undo_stack has entries?
       │
       ├── Pop UndoEntry from undo_stack
       │
       ├── Push current state to redo_stack
       │
       ├── Restore state from UndoEntry
       │
       ├── Validate invariants (should always pass)
       │
       └── Update version, emit STATE_UPDATE
```

---

## 6. Persistence & Recovery Model

### 6.1 Storage Components

| Component | Purpose | Location |
|-----------|---------|----------|
| **Event Store** | Append-only log of all EditActions | `persistence/event_store.rs` |
| **Snapshot Store** | Periodic full state snapshots | `persistence/snapshot_store.rs` |
| **Write-Ahead Log** | Uncommitted events for crash recovery | `persistence/wal.rs` |
| **Recovery Engine** | Crash-safe state reconstruction | `engine/recovery.rs` |

### 6.2 Event Store

- Every `EditAction` is appended with version number
- Events are marked committed only after successful mutation
- Uncommitted events are discarded on recovery

### 6.3 Snapshot Strategy

```rust
const SNAPSHOT_INTERVAL: u64 = 50; // Snapshot every 50 events
```

- Full `TimelineState` serialized to disk
- Includes version number for correlation with events
- Retention policy prunes old snapshots

### 6.4 Recovery Algorithm

```text
1. LOAD STARTING STATE
   - Find newest valid snapshot
   - If no valid snapshot: start with empty state

2. LOAD COMMITTED EVENTS
   - Filter: committed = true AND version > snapshot.version
   - Sort by version (ascending)

3. REPLAY EVENTS
   - Apply each event to state
   - Skip failed events (log warning)

4. VALIDATE INVARIANTS
   - Run full validation on recovered state
   - Fail recovery if invariants violated

5. RETURN RECOVERED STATE
   - Include metadata: events_replayed, snapshot_version
```

### 6.5 Deterministic Replay Guarantee

Given:
- Identical starting snapshot
- Identical event sequence
- Identical event application order

Result: **Byte-identical final state**

Requirements:
- Immutable ClipIds (UUIDs generated at creation)
- Fixed frame rate
- No randomness in mutation logic
- Deterministic FFmpeg parameters

---

## 7. Testing & Reliability Strategy

### 7.1 Test Categories

| Category | Purpose | Location |
|----------|---------|----------|
| **Invariant Tests** | Verify all invariants are enforced | `tests/invariants_tests.rs` |
| **Engine Invariant Tests** | Test engine-level invariant checking | `tests/invariants_engine_tests.rs` |
| **Performance Tests** | Ensure O(1)/O(log n) complexity | `tests/performance_tests.rs` |
| **Persistence Tests** | Event store, snapshots, recovery | `tests/persistence_integration_tests.rs` |
| **Undo/Redo Tests** | Verify undo stack behavior | `tests/undo_redo_tests.rs` |
| **Edit Plan Tests** | AI edit plan validation | `tests/edit_plan_tests.rs` |
| **Prompt Tests** | LLM prompt generation | `tests/prompt_tests.rs` |

### 7.2 What Failures They Prevent

| Test | Prevents |
|------|----------|
| Unique ClipId validation | Duplicate ID corruption |
| Overlap detection | Undefined rendering behavior |
| Duration/start validation | Invalid clip state |
| Performance regression | O(n²) slowdowns at scale |
| Recovery tests | Data loss on crash |
| Undo consistency | State divergence after undo |
| AI safety tests | Malicious LLM output injection |

### 7.3 Performance Targets

| Operation | Target | Measured @ 5000 clips |
|-----------|--------|----------------------|
| `get_clip_by_id()` | < 1µs | ~200ns ✅ |
| `find_clip_at_time()` | < 20µs | ~1-2µs ✅ |
| `would_overlap()` | < 10µs | ~1-3µs ✅ |
| `validate_invariants()` | < 50ms | ~15-20ms ✅ |
| Single action | < 20ms | ~5-10ms ✅ |

### 7.4 Running Tests

```bash
# All tests
cargo test

# Performance tests in release mode
cargo test --test performance_tests --release

# Specific test category
cargo test invariants
cargo test persistence_integration
```

---

## 8. How to Work in This Codebase

### 8.1 Adding New Engine Features

1. **Define the action** in `engine/edit_action.rs`:
   ```rust
   pub enum ActionType {
       // ... existing actions
       NewAction { param: Type },
   }
   ```

2. **Implement mutation** in `TimelineEngine::execute_mutation()`:
   ```rust
   ActionType::NewAction { param } => {
       // Mutate state
       // Invariants are validated AFTER this returns
   }
   ```

3. **Add tests** in `tests/` demonstrating the action works and invariants hold

4. **Wire to orchestrator** if needed in `AppOrchestrator::apply_timeline_action()`

### 8.2 Adding Commands (Keyboard Shortcuts)

1. **Define command ID** in `engine/commands/command.rs`:
   ```rust
   pub mod commands {
       pub const NEW_COMMAND: &str = "edit.newCommand";
   }
   ```

2. **Register in CommandRegistry** (via `register_default_commands`):
   ```rust
   registry.register(CommandDescriptor {
       id: CommandId::new(commands::NEW_COMMAND),
       name: "New Command",
       category: CommandCategory::Edit,
   });
   ```

3. **Add keybinding** in `engine/commands/keymap.rs`:
   ```rust
   keymap.bind(KeyBinding::ctrl('n'), CommandId::new(commands::NEW_COMMAND));
   ```

4. **Implement handler** in `CommandRouter::execute_command()`

### 8.3 Adding Tools

1. **Add tool type** in `engine/interaction/tools.rs`:
   ```rust
   pub enum ToolType {
       // ... existing tools
       NewTool,
   }
   ```

2. **Handle in InteractionController**:
   - `on_mouse_down()` - determine interaction target
   - `on_mouse_move()` - update preview state
   - `on_mouse_up()` - generate EditAction or cancel

3. **Add preview state** if tool shows live feedback during drag

### 8.4 Adding UI Panels

1. **Define panel type** in `engine/ui/composition/`:
   ```rust
   pub enum PanelType {
       // ... existing panels
       NewPanel,
   }
   ```

2. **Register panel descriptor** in panel registry

3. **Add React component** in `src/components/`

4. **Wire to WorkspaceEngine** for show/hide/move commands

### 8.5 Modifying the Orchestrator

⚠️ **Caution**: The orchestrator is the central coordination point.

1. **Maintain atomic cross-engine effects** - if one fails, all must rollback
2. **Never let engines call each other** - always route through orchestrator
3. **Update `apply()` dispatch** if adding new AppCommand variants
4. **Test compound command behavior**

### 8.6 What NOT to Do

| ❌ Don't | ✅ Do Instead |
|---------|--------------|
| Mutate state outside `apply_action()` | Use `TimelineEngine::apply_action()` |
| Add business logic to React | Keep React display-only; send intents to backend |
| Let engines call each other directly | Route all cross-engine communication through orchestrator |
| Trust LLM output | Always wrap in `UntrustedAIResponse`, use AI pipeline |
| Store derived data as authoritative | Rebuild indexes from source data |
| Skip invariant validation | Always validate after mutations |
| Use `unsafe` code | All Rust code is safe Rust |
| Add local timeline state in React | `STATE_UPDATE` is the only source of truth |

---

## 9. Roadmap Snapshot

### Completed Phases

- ✅ **Phase A**: Core Timeline Engine & God State
- ✅ **Phase B**: Performance Indexing (HashMap, BTreeMap)
- ✅ **Phase C**: Invariant Validation System
- ✅ **Phase D**: Event Sourcing & Persistence
- ✅ **Phase E**: Crash Recovery
- ✅ **Phase F**: AI Pipeline Hardening (7-stage)
- ✅ **Phase G**: Playback Infrastructure
- ✅ **Phase H**: Audio/Video Sync
- ✅ **Phase I**: Render Pipeline
- ✅ **Phase J**: Workspace Engine
- ✅ **Phase K**: Interaction System
- ✅ **Phase L**: Command System
- ✅ **Phase M**: UI Bridge

### Planned Phases

- ⏳ **Phase N**: Multi-Track Support
  - Track model (video, audio, overlay tracks)
  - Per-track overlap rules
  - Track-based routing

- ⏳ **Phase O**: Effects & Transitions
  - Effect graph model
  - Transition types
  - Keyframe animation

- ⏳ **Phase P**: Media Management
  - Media pool
  - Proxy generation
  - Codec support expansion

- ⏳ **Phase Q**: Export Pipeline
  - FFmpeg integration hardening
  - Preset system
  - Batch export

- ⏳ **Phase R**: LLM Enhancement
  - Multi-model support
  - Fine-tuned editing prompts
  - Streaming responses

---

## Appendix A: Key File Locations

| System | Location |
|--------|----------|
| App Entry | `src-tauri/src/lib.rs` |
| Engine Root | `src-tauri/src/engine/mod.rs` |
| TimelineEngine | `src-tauri/src/engine/timeline_engine.rs` |
| AppOrchestrator | `src-tauri/src/engine/orchestrator/orchestrator.rs` |
| AI Pipeline | `src-tauri/src/engine/ai_pipeline.rs` |
| Invariants | `src-tauri/src/engine/invariants.rs` |
| Recovery | `src-tauri/src/engine/recovery.rs` |
| Playback | `src-tauri/src/engine/playback/` |
| Audio | `src-tauri/src/engine/audio/` |
| Render | `src-tauri/src/engine/render/` |
| Workspace | `src-tauri/src/engine/workspace/` |
| Interaction | `src-tauri/src/engine/interaction/` |
| Commands | `src-tauri/src/engine/commands/` |
| UI Bridge | `src-tauri/src/engine/ui/` |
| Persistence | `src-tauri/src/persistence/` |
| Tests | `src-tauri/tests/` |
| React Frontend | `src/` |

---

## Appendix B: Glossary

| Term | Definition |
|------|------------|
| **God State** | Single source of truth pattern; TimelineEngine owns all mutable state |
| **Single Mutation Path** | All changes via `apply_action()`; no backdoor mutations |
| **Invariant** | Rule that must hold for valid state; checked on every mutation |
| **Snapshot** | Immutable clone of state at a point in time |
| **Event Sourcing** | Storing sequence of actions rather than current state |
| **A/V Sync** | Keeping video timing aligned with audio (audio is master) |
| **Preflight** | Simulating edits before applying to detect conflicts |
| **MediaTime** | Integer-based time representation for frame-accurate operations |
| **ClipId** | UUID identifying a clip; never reused, even after deletion |
| **UntrustedAIResponse** | Wrapper type ensuring all LLM output goes through validation |

---

*Document Version: 1.0*  
*Last Updated: 2026-01-04*  
*Maintainer: Antigravity Engineering Team*
