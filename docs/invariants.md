# Antigravity Invariants

> The constitutional rulebook of the Antigravity Video Engine.

## Overview

Invariants are non-negotiable rules that must hold true for any valid `TimelineState`. They are enforced programmatically by `validate_invariants()` and checked on every mutation path.

---

## Constitutional Invariants

### 1. Clip Identity & Immutability

**Rule:** All `ClipId` values are globally unique within a timeline.

```
∀ clip₁, clip₂ ∈ clips: clip₁.id ≠ clip₂.id (when clip₁ ≠ clip₂)
```

- ClipIds are UUIDs generated at clip creation
- **SPLIT operations always create new ClipIds** for the resulting segments
- ClipIds are never reused, even after deletion
- This enables reliable undo, AI tracking, and export determinism

### 2. Timeline vs Project Duration

**Rule:** `project_duration >= max(clip.start + clip.duration)`

```
project_duration >= content_duration
where content_duration = max(clip.end() for clip in clips)
```

- `project_duration` defines the exportable timeline length
- It may exceed content to allow empty space at the end
- Recalculated automatically when clips change

### 3. Frame Rate Authority

**Rule:** Exactly one `timeline_frame_rate` exists per project.

```
timeline_frame_rate > 0
```

- Stored in `ProjectSettings.timeline_frame_rate`
- All time calculations use this authoritative frame rate
- Default: 30.0 fps
- Changing frame rate affects all time-to-frame conversions

### 4. Clip Duration Validity

**Rule:** All clips have positive duration.

```
∀ clip ∈ clips: clip.duration > 0
```

- Zero-duration clips are invalid
- Negative duration is a fatal error
- Minimum practical duration: 1 frame (1/fps seconds)

### 5. Clip Start Validity

**Rule:** All clips have non-negative start time.

```
∀ clip ∈ clips: clip.start >= 0
```

- No clip can start before the timeline origin
- Gaps before first clip are allowed

### 6. Playhead Bounds

**Rule:** Playhead position is within project bounds.

```
playhead_time ∈ [0, project_duration]
```

- Playhead cannot be negative
- Playhead cannot exceed project duration
- Clamped automatically on duration changes

---

## Track Stability (Future)

**Planned Invariant:** All `clip.track_id` reference existing tracks.

```
∀ clip ∈ clips: clip.track_id ∈ tracks.keys()
```

Currently using single-track V1 mode with implicit track existence.

---

## Time Precision Policy

**Policy:** All time values use `f64` seconds with 3-decimal (millisecond) precision for comparisons.

```rust
const EPSILON: f64 = 0.001; // 1ms tolerance
```

- Floating-point comparisons use tolerance
- Sub-millisecond precision preserved internally
- Frame-accurate seeking: `frame = floor(time * fps)`

---

## Export Determinism

**Guarantee:** Given identical `TimelineState` and export settings, the export produces byte-identical output.

Requirements for determinism:
- Immutable ClipIds ensure clip ordering stability
- Fixed frame rate prevents timing drift
- No randomness in render pipeline
- Identical FFmpeg parameters

---

## Enforcement

Invariants are enforced by:

```rust
impl TimelineState {
    pub fn validate_invariants(&self) -> Result<(), String>;
}
```

**Called by:**
- `action_router::run_edit_plan()` after mutations
- Future: Every state mutation wrapper

**On Violation:**
- Returns `Err(String)` describing the violation
- State mutation is rolled back (atomic transactions)
- UI displays user-friendly error

---

## Error Codes

| Error Prefix | Meaning |
|--------------|---------|
| `INVARIANT_VIOLATED: Duplicate ClipId` | Two clips have same ID |
| `INVARIANT_VIOLATED: project_duration` | Duration constraint violated |
| `INVARIANT_VIOLATED: timeline_frame_rate` | Invalid frame rate |
| `INVARIANT_VIOLATED: Clip '...' has invalid duration` | Zero/negative clip duration |
| `INVARIANT_VIOLATED: Clip '...' has negative start` | Clip starts before timeline origin |
| `INVARIANT_VIOLATED: playhead_time` | Playhead out of bounds |
