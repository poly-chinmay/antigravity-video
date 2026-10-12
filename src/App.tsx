import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import MissionControl from "./components/MissionControl";
import MediaPoolPanel from "./components/MediaPoolPanel";
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

// MediaSource from import_media_file
interface MediaSource {
  id: string;
  path: string;
  duration_secs: number;
  width: number;
  height: number;
  frame_rate: number;
  video_codec: string;
  audio_codec: string | null;
  file_size: number;
  display_name: string;
}

function App() {
  const [timelineState, setTimelineState] = useState<TimelineState | null>(null);
  const [isPlaying, setIsPlaying] = useState<boolean>(false);
  const [importStatus, setImportStatus] = useState<string | null>(null);
  const [canUndo, setCanUndo] = useState<boolean>(false);
  const [canRedo, setCanRedo] = useState<boolean>(false);

  // Check undo/redo availability
  const checkUndoRedo = useCallback(async () => {
    try {
      const [undoAvail, redoAvail] = await Promise.all([
        invoke<boolean>('can_undo'),
        invoke<boolean>('can_redo'),
      ]);
      setCanUndo(undoAvail);
      setCanRedo(redoAvail);
    } catch (error) {
      console.error('Failed to check undo/redo:', error);
    }
  }, []);

  // Undo action
  const handleUndo = useCallback(async () => {
    try {
      await invoke('undo_command');
      console.log('↩️ Undo executed');
    } catch (error) {
      console.error('Undo failed:', error);
    }
  }, []);

  // Redo action
  const handleRedo = useCallback(async () => {
    try {
      await invoke('redo_command');
      console.log('↪️ Redo executed');
    } catch (error) {
      console.error('Redo failed:', error);
    }
  }, []);

  // Seek to a specific time
  const seekTo = useCallback(async (time: number) => {
    try {
      await invoke("seek_timeline", { time });
    } catch (error) {
      console.error("Failed to seek:", error);
    }
  }, []);

  useEffect(() => {
    console.log("🚀 [Frontend] App mounted. Setting up listeners...");

    // Handle file drops
    const unlisten = listen<{ paths: string[] }>("tauri://drop", async (event) => {
      console.log("📂 [Frontend] File dropped:", event.payload.paths);
      for (const path of event.payload.paths) {
        if (path.match(/\.(mp4|mov|avi|mkv|webm)$/i)) {
          await importToPool(path);
        } else {
          console.warn("⚠️ Ignored non-video file:", path);
        }
      }
    });

    // STATE_UPDATE listener
    const unlistenState = listen<TimelineState>("STATE_UPDATE", (event) => {
      console.log("⚡️ [Frontend] Received STATE_UPDATE event:", event.payload);
      setTimelineState(event.payload);
      // Check undo/redo availability after state update
      checkUndoRedo();
    });

    // Keyboard shortcuts
    const handleKeyDown = (e: KeyboardEvent) => {
      const isMac = navigator.platform.toUpperCase().indexOf('MAC') >= 0;
      const modKey = isMac ? e.metaKey : e.ctrlKey;

      if (modKey && e.key === 'z' && !e.shiftKey) {
        e.preventDefault();
        handleUndo();
      } else if (modKey && e.key === 'z' && e.shiftKey) {
        e.preventDefault();
        handleRedo();
      } else if (modKey && e.key === 'y') {
        // Windows redo shortcut
        e.preventDefault();
        handleRedo();
      }
    };

    window.addEventListener('keydown', handleKeyDown);

    // Initial undo/redo check
    checkUndoRedo();

    return () => {
      unlisten.then((f) => f());
      unlistenState.then((f) => f());
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [checkUndoRedo, handleUndo, handleRedo]);

  // Pool-first import: adds to MediaRegistry, NOT timeline
  async function importToPool(filePath: string) {
    setImportStatus("Importing...");
    try {
      console.log("🚀 [Frontend] Importing to pool:", filePath);
      const source = await invoke<MediaSource>("import_media_file", { path: filePath });
      console.log("✅ [Frontend] Added to pool:", source.display_name);
      setImportStatus(`Added to pool: ${source.display_name}`);
      setTimeout(() => setImportStatus(null), 3000);
    } catch (error) {
      console.error("❌ [Frontend] Import failed:", error);
      setImportStatus(`Import failed: ${error}`);
      setTimeout(() => setImportStatus(null), 5000);
    }
  }

  // Handle Import Video button click
  async function handleImportClick() {
    console.log("🖱️ [Frontend] Import Video clicked");
    try {
      const selected = await open({
        multiple: false,
        filters: [{
          name: 'Video',
          extensions: ['mp4', 'mov', 'avi', 'mkv', 'webm']
        }]
      });

      if (selected) {
        await importToPool(selected as string);
      }
    } catch (error) {
      console.error("❌ [Frontend] File picker failed:", error);
    }
  }

  // Callback when clip is added from MediaPool
  const handleClipAdded = useCallback((_clipId: string, fileName: string) => {
    console.log(`🎬 [Frontend] Clip added from pool: ${fileName}`);
    // Timeline updates via STATE_UPDATE event
  }, []);

  return (
    <div className="app-container">
      <div className="left-panel">
        <h1 style={{ fontSize: '1.5rem', fontWeight: 'bold', marginBottom: '20px', color: 'var(--accent-color)' }}>
          Ghost Engine
        </h1>
        <MissionControl />

        {/* Undo/Redo Buttons */}
        <div style={{ marginTop: '20px', display: 'flex', gap: '8px' }}>
          <button
            className="btn-secondary"
            onClick={handleUndo}
            disabled={!canUndo}
            title="Undo (Cmd+Z)"
            style={{ opacity: canUndo ? 1 : 0.5, cursor: canUndo ? 'pointer' : 'not-allowed' }}
          >
            ↩ Undo
          </button>
          <button
            className="btn-secondary"
            onClick={handleRedo}
            disabled={!canRedo}
            title="Redo (Cmd+Shift+Z)"
            style={{ opacity: canRedo ? 1 : 0.5, cursor: canRedo ? 'pointer' : 'not-allowed' }}
          >
            ↪ Redo
          </button>
        </div>

        <div style={{ marginTop: '20px', display: 'flex', flexDirection: 'column', gap: '10px' }}>
          <h3 style={{ fontSize: '0.9rem', color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: '1px' }}>
            Import
          </h3>
          <button className="btn-secondary" onClick={handleImportClick}>
            + Import Video
          </button>

          {/* Import Status */}
          {importStatus && (
            <div style={{
              padding: '8px 12px',
              fontSize: '0.85rem',
              backgroundColor: importStatus.startsWith('Import failed')
                ? 'rgba(239, 68, 68, 0.2)'
                : 'rgba(34, 197, 94, 0.2)',
              borderRadius: '4px',
            }}>
              {importStatus}
            </div>
          )}
        </div>

        {/* Media Pool Panel */}
        <div style={{ marginTop: '20px' }}>
          <MediaPoolPanel onClipAdded={handleClipAdded} />
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