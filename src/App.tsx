import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import MissionControl from "./components/MissionControl";
import Timeline from "./components/Timeline";
import VideoPlayer from "./components/VideoPlayer";

interface Clip {
  id: string;
  source_file: string;
  start: number;
  duration: number;
  track_id: string;
}

interface TimelineState {
  clips: Clip[];
  duration: number;
  playhead_time: number;
  version: number;
}

function App() {
  const [timelineState, setTimelineState] = useState<TimelineState | null>(null);
  // STEP 1 FIX: playheadTime removed - now derived from timelineState.playhead_time
  // isPlaying is frontend-only UI state (allowed per execution order)
  const [isPlaying, setIsPlaying] = useState<boolean>(false);

  // STEP 2 FIX: fetchState removed - STATE_UPDATE is the ONLY source of truth

  // Seek to a specific time - STEP 1 FIX: No local state update, backend is source of truth
  const seekTo = useCallback(async (time: number) => {
    // Call backend, which will emit STATE_UPDATE with new playhead_time
    try {
      await invoke("seek_timeline", { time });
      // STATE_UPDATE listener will update timelineState with new playhead_time
    } catch (error) {
      console.error("Failed to seek:", error);
    }
  }, []);

  useEffect(() => {
    console.log("🚀 [Frontend] App mounted. Setting up listeners...");
    // STEP 2 FIX: No fetchState() call - backend emits initial STATE_UPDATE on app ready

    const unlisten = listen<{ paths: string[] }>("tauri://drop", (event) => {
      console.log("📂 [Frontend] File dropped:", event.payload.paths);
      for (const path of event.payload.paths) {
        // Simple extension check
        if (path.match(/\.(mp4|mov|avi|mkv|webm)$/i)) {
          invoke("import_video", { filePath: path })
            .then(() => console.log("✅ Imported:", path))
            .catch((e) => console.error("❌ Import failed:", e));
        } else {
          console.warn("⚠️ Ignored non-video file:", path);
        }
      }
    });

    // STEP 2: This is the ONLY way frontend receives state
    const unlistenState = listen<TimelineState>("STATE_UPDATE", (event) => {
      console.log("⚡️ [Frontend] Received STATE_UPDATE event:", event.payload);
      setTimelineState(event.payload);
    });

    return () => {
      unlisten.then((f) => f());
      unlistenState.then((f) => f());
    };
  }, []);

  async function importVideo() {
    console.log("🖱️ [Frontend] Import Video clicked");
    try {
      const selected = await open({
        multiple: false,
        filters: [{
          name: 'Video',
          extensions: ['mp4', 'mov', 'avi', 'mkv', 'webm']
        }]
      });
      console.log("📂 [Frontend] Selected file:", selected);

      if (selected) {
        console.log("🚀 [Frontend] Invoking import_video...");
        await invoke("import_video", { filePath: selected });
        console.log("✅ [Frontend] Import successful");
        // STEP 2 FIX: No fetchState() - backend emits STATE_UPDATE after import
      }
    } catch (error) {
      console.error("❌ [Frontend] Import failed:", error);
      alert(`Import failed: ${error}`);
    }
  }

  async function addTestClips() {
    console.log("🖱️ [Frontend] Add Test Clips clicked");
    try {
      console.log("🚀 [Frontend] Invoking add_test_clips...");
      await invoke("add_test_clips", { count: 3 });
      console.log("✅ [Frontend] Test clips added");
    } catch (error) {
      console.error("❌ [Frontend] Failed to add test clips:", error);
      alert(`Failed to add test clips: ${error}`);
    }
  }

  return (
    <div className="app-container">
      <div className="left-panel">
        <h1 style={{ fontSize: '1.5rem', fontWeight: 'bold', marginBottom: '20px', color: 'var(--accent-color)' }}>
          Ghost Engine
        </h1>
        <MissionControl />

        <div style={{ marginTop: '20px', display: 'flex', flexDirection: 'column', gap: '10px' }}>
          <h3 style={{ fontSize: '0.9rem', color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: '1px' }}>
            Library
          </h3>
          <button className="btn-secondary" onClick={importVideo}>
            + Import Video
          </button>
          <button className="btn-secondary" onClick={addTestClips}>
            + Add 3 Test Clips
          </button>
        </div>
      </div>

      <div className="right-panel">
        <div className="preview-area">
          <VideoPlayer
            clips={timelineState?.clips || []}
            playheadTime={timelineState?.playhead_time ?? 0}
            onPlayheadChange={seekTo}
            isPlaying={isPlaying}
            onPlayingChange={setIsPlaying}
          />
        </div>
      </div>

      <Timeline
        timelineState={timelineState}
        playheadTime={timelineState?.playhead_time ?? 0}
        onSeek={seekTo}
      />
    </div>
  );
}

export default App;