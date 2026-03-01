import { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";

interface Clip {
    id: string;
    source_file: string;
    start: number;
    duration: number;
    track_id: string;
}

interface VideoPlayerProps {
    clips: Clip[];
    playheadTime: number;
    onPlayheadChange: (time: number) => void;
    isPlaying: boolean;
    onPlayingChange: (playing: boolean) => void;
}

export default function VideoPlayer({
    clips,
    playheadTime,
    onPlayheadChange,
    isPlaying,
    onPlayingChange,
}: VideoPlayerProps) {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const [previewPath, setPreviewPath] = useState<string | null>(null);
    const [isLoading, setIsLoading] = useState<boolean>(false);
    const lastRequestTime = useRef<number>(0);
    const debounceTimer = useRef<number | null>(null);

    // FETCH FRAME LOGIC
    useEffect(() => {
        const fetchFrame = async () => {
            const now = Date.now();
            if (now - lastRequestTime.current < 50) {
                if (debounceTimer.current) clearTimeout(debounceTimer.current);
                debounceTimer.current = window.setTimeout(fetchFrame, 50);
                return;
            }
            lastRequestTime.current = now;

            setIsLoading(true);
            try {
                const overClip = clips.some(c =>
                    playheadTime >= c.start && playheadTime < c.start + c.duration
                );

                if (overClip) {
                    const path = await invoke<string>("render_preview_frame", { timeSecs: playheadTime });
                    setPreviewPath(path);
                } else {
                    setPreviewPath(null);
                }
            } catch (err) {
                console.error("Preview render failed:", err);
                if (String(err).includes("No clip")) {
                    setPreviewPath(null);
                }
            } finally {
                setIsLoading(false);
            }
        };

        fetchFrame();
    }, [playheadTime, clips]);

    // RENDER LOGIC (Blob URL Strategy)
    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const ctx = canvas.getContext('2d');
        if (!ctx) return;

        let activeUrl: string | null = null;

        const loadAndDraw = async () => {
            // 1. Reset/Clear if no path
            if (!previewPath) {
                ctx.fillStyle = '#000';
                ctx.fillRect(0, 0, canvas.width, canvas.height);
                ctx.fillStyle = '#333';
                ctx.font = '24px Inter';
                ctx.textAlign = 'center';
                ctx.fillText("No Signal", canvas.width / 2, canvas.height / 2);
                return;
            }

            try {
                // 2. Read file to Blob (Bypass asset:// protocol)
                console.log("🖼️ [VideoPlayer] Reading Blob:", previewPath);
                const bytes = await readFile(previewPath);
                const blob = new Blob([bytes], { type: 'image/png' });
                activeUrl = URL.createObjectURL(blob);

                // 3. Load Image
                const img = new Image();
                img.onload = () => {
                    console.log("✅ [VideoPlayer] Loaded Blob:", img.width, "x", img.height);

                    const c = canvasRef.current;
                    if (!c) return;
                    const context = c.getContext('2d');
                    if (!context) return;

                    context.fillStyle = '#000';
                    context.fillRect(0, 0, c.width, c.height);

                    const scale = Math.min(c.width / img.width, c.height / img.height);
                    const x = (c.width / 2) - (img.width / 2) * scale;
                    const y = (c.height / 2) - (img.height / 2) * scale;

                    context.drawImage(img, x, y, img.width * scale, img.height * scale);
                };

                img.onerror = (e) => {
                    console.error("❌ [VideoPlayer] Blob load failed:", e);
                };

                img.src = activeUrl;
            } catch (fsErr) {
                console.error("❌ [VideoPlayer] FS Read Failed:", fsErr);
            }
        };

        loadAndDraw();

        // Cleanup Blob URL
        return () => {
            if (activeUrl) {
                URL.revokeObjectURL(activeUrl);
            }
        };

    }, [previewPath]);

    // Controls Logic
    const togglePlay = () => {
        if (clips.length === 0) return;
        const totalDuration = clips.reduce((max, c) => Math.max(max, c.start + c.duration), 0);
        if (!isPlaying && playheadTime >= totalDuration - 0.1) {
            onPlayheadChange(0);
        }
        onPlayingChange(!isPlaying);
    };

    if (clips.length === 0) {
        return (
            <div className="video-player-container placeholder">
                <div className="placeholder-content">
                    <span style={{ fontSize: "2rem", marginBottom: "10px" }}>🎬</span>
                    <p>No video loaded yet</p>
                </div>
            </div>
        );
    }

    return (
        <div className="video-player-container">
            <canvas
                ref={canvasRef}
                width={1280}
                height={720}
                className="video-element"
                style={{ backgroundColor: 'black' }}
            />

            {isLoading && (
                <div style={{
                    position: 'absolute', top: '20px', right: '20px',
                    width: '10px', height: '10px', borderRadius: '50%',
                    backgroundColor: 'var(--accent-color)',
                    boxShadow: '0 0 10px var(--accent-color)'
                }} />
            )}

            <div className="video-overlay">
                <span className="video-path">
                    FFmpeg Preview {isLoading ? "(Rendering...)" : ""}
                </span>
            </div>

            <div className="video-controls">
                <button className="btn-play" onClick={togglePlay}>
                    {isPlaying ? "⏸" : "▶"}
                </button>
                <span className="time-display">
                    {playheadTime.toFixed(2)}s
                </span>
            </div>
        </div>
    );
}
