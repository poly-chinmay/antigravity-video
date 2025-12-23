import { useState, useEffect } from "react";
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
}

function App() {
  const [timelineState, setTimelineState] = useState<TimelineState | null>(null);
  const [previewPath, setPreviewPath] = useState<string | null>(null);

  // Function to fetch the latest state from the backend
  async function fetchState() {
    try {
      const state = await invoke<TimelineState>("get_timeline_state");
      setTimelineState(state);
    } catch (error) {
      console.error("Failed to fetch state:", error);
    }
  }

  useEffect(() => {
    console.log("🚀 [Frontend] App mounted. Setting up listeners...");
    fetchState();

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

    // Listen for backend state updates
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
        fetchState();
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
        <MissionControl onPreviewReady={setPreviewPath} />

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
          <VideoPlayer clips={timelineState?.clips || []} />
        </div>
      </div>

      <Timeline timelineState={timelineState} />
    </div>
  );
}

export default App;