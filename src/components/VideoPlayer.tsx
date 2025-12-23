import { useEffect, useState, useRef, useCallback } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

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

/**
 * Get the clip that should be active at a given timeline time.
 * Returns the clip where: clip.start <= time < clip.start + clip.duration
 */
function getActiveClip(clips: Clip[], time: number): Clip | null {
    // Sort by start time to ensure correct order
    const sorted = [...clips].sort((a, b) => a.start - b.start);
    for (const clip of sorted) {
        if (clip.start <= time && time < clip.start + clip.duration) {
            return clip;
        }
    }
    return null;
}

/**
 * VideoPlayer - Timeline-aware video playback
 * 
 * V1 Implementation (Source-Based Preview):
 * - Uses playheadTime to determine which clip should be visible
 * - Switches video source when clip boundaries are crossed
 * - Syncs video.currentTime with timeline position relative to clip start
 * 
 * V1 LIMITATIONS (STEP 6 - Explicit Constraints):
 * CAN DO:
 *   - Play source files at computed offsets
 *   - Switch source when clip boundary crossed
 *   - Show placeholder for gaps
 *   - Pause/play based on UI toggle
 *   - Handle clip deletion (backend clamps playhead)
 * 
 * CANNOT DO (Forbidden in V1):
 *   - Transitions between clips (no transition data in state)
 *   - Effects/filters (no effect pipeline)
 *   - Audio mixing (no audio track model)
 *   - Smooth clip-to-clip handoff (requires pre-render)
 *   - Sub-frame accurate seeking (HTML5 video limitation)
 * 
 * KNOWN V1 LIMITATION - SOURCE OFFSET:
 *   Trimmed/split clips do NOT play from correct source offset.
 *   Clip struct lacks `source_offset` field - would require data model change.
 *   Workaround: Export renders correctly; preview shows wrong segment.
 * 
 * V2 FUTURE: Add source_offset to Clip, backend-rendered preview segments.
 */
export default function VideoPlayer({
    clips,
    playheadTime,
    onPlayheadChange,
    isPlaying,
    onPlayingChange,
}: VideoPlayerProps) {
    const videoRef = useRef<HTMLVideoElement>(null);
    const [error, setError] = useState<string | null>(null);
    const [currentClipId, setCurrentClipId] = useState<string | null>(null);
    const animationFrameRef = useRef<number | null>(null);

    // Compute active clip based on playhead position
    const activeClip = getActiveClip(clips, playheadTime);

    // Update video source when active clip changes
    useEffect(() => {
        if (!activeClip) {
            setCurrentClipId(null);
            return;
        }

        // Only reload if clip actually changed
        if (activeClip.id !== currentClipId) {
            console.log("🎬 [VideoPlayer] Switching to clip:", activeClip.id);
            setCurrentClipId(activeClip.id);
            setError(null);

            if (videoRef.current) {
                const url = convertFileSrc(activeClip.source_file);
                console.log("🔗 [VideoPlayer] Loading:", url);
                videoRef.current.src = url;

                // V1: Compute offset based on timeline position
                // NOTE: This does NOT account for source_offset (trimmed clips)
                // Trimmed/split clips will play from wrong source position
                const offsetInClip = playheadTime - activeClip.start;
                const clampedOffset = Math.max(0, offsetInClip);
                console.log(`📍 [VideoPlayer] Seeking to ${clampedOffset.toFixed(2)}s in source (V1: no source_offset)`);
                videoRef.current.currentTime = clampedOffset;

                if (isPlaying) {
                    videoRef.current.play().catch(e => console.error("Autoplay failed:", e));
                }
            }
        }
    }, [activeClip?.id, currentClipId, isPlaying]);

    // Sync video position when playhead changes externally (e.g., seek from timeline)
    useEffect(() => {
        if (!activeClip || !videoRef.current) return;

        const offsetInClip = playheadTime - activeClip.start;
        const currentVideoTime = videoRef.current.currentTime;

        // Only seek if there's a significant difference (avoid micro-corrections during playback)
        if (Math.abs(currentVideoTime - offsetInClip) > 0.1) {
            videoRef.current.currentTime = Math.max(0, offsetInClip);
        }
    }, [playheadTime, activeClip?.start]);

    // Update playhead during video playback
    const updatePlayhead = useCallback(() => {
        if (!videoRef.current || !activeClip || !isPlaying) return;

        const videoTime = videoRef.current.currentTime;
        const newPlayheadTime = activeClip.start + videoTime;

        // Check if we've reached the end of the current clip
        if (videoTime >= activeClip.duration) {
            // Find next clip
            const nextClip = getActiveClip(clips, activeClip.start + activeClip.duration + 0.01);
            if (nextClip) {
                console.log("🎬 [VideoPlayer] Auto-advancing to next clip");
                onPlayheadChange(nextClip.start);
            } else {
                // End of timeline
                console.log("🛑 [VideoPlayer] End of timeline reached");
                onPlayingChange(false);
                return;
            }
        } else {
            onPlayheadChange(newPlayheadTime);
        }

        // Continue animation loop
        if (isPlaying) {
            animationFrameRef.current = requestAnimationFrame(updatePlayhead);
        }
    }, [activeClip, clips, isPlaying, onPlayheadChange, onPlayingChange]);

    // Start/stop playback loop
    useEffect(() => {
        if (isPlaying && videoRef.current) {
            videoRef.current.play().catch(e => console.error("Play failed:", e));
            animationFrameRef.current = requestAnimationFrame(updatePlayhead);
        } else if (videoRef.current) {
            videoRef.current.pause();
            if (animationFrameRef.current) {
                cancelAnimationFrame(animationFrameRef.current);
            }
        }

        return () => {
            if (animationFrameRef.current) {
                cancelAnimationFrame(animationFrameRef.current);
            }
        };
    }, [isPlaying, updatePlayhead]);

    // Handle play/pause toggle
    const togglePlay = () => {
        if (clips.length === 0) return;

        // If at end of timeline, restart
        const totalDuration = clips.reduce((max, c) => Math.max(max, c.start + c.duration), 0);
        if (!isPlaying && playheadTime >= totalDuration - 0.1) {
            onPlayheadChange(0);
        }

        onPlayingChange(!isPlaying);
    };

    // No clips loaded
    if (clips.length === 0) {
        return (
            <div className="video-player-container placeholder">
                <div className="placeholder-content">
                    <span style={{ fontSize: "2rem", marginBottom: "10px" }}>🎬</span>
                    <p>No video loaded yet</p>
                    <small>Import a video to start editing</small>
                </div>
            </div>
        );
    }

    // Gap in timeline (no clip at current position)
    if (!activeClip) {
        return (
            <div className="video-player-container placeholder">
                <div className="placeholder-content">
                    <span style={{ fontSize: "2rem", marginBottom: "10px" }}>⏸️</span>
                    <p>Gap in timeline</p>
                    <small>Position: {playheadTime.toFixed(2)}s</small>
                    <button
                        className="btn-primary"
                        onClick={togglePlay}
                        style={{ marginTop: "10px" }}
                    >
                        {isPlaying ? "⏸ Pause" : "▶ Play"}
                    </button>
                </div>
            </div>
        );
    }

    // Error state
    if (error) {
        return (
            <div className="video-player-container error">
                <div className="error-content">
                    <p>Failed to load video</p>
                    <small>{error}</small>
                    <small style={{ marginTop: "10px", opacity: 0.7 }}>
                        Path: {activeClip.source_file}
                    </small>
                </div>
            </div>
        );
    }

    return (
        <div className="video-player-container">
            <video
                ref={videoRef}
                className="video-element"
                onError={(e) => {
                    const target = e.target as HTMLVideoElement;
                    const mediaError = target.error;
                    console.error("Video Error Event:", e);
                    let msg = "Playback failed.";
                    if (mediaError) {
                        switch (mediaError.code) {
                            case MediaError.MEDIA_ERR_ABORTED: msg += " Aborted."; break;
                            case MediaError.MEDIA_ERR_NETWORK: msg += " Network error."; break;
                            case MediaError.MEDIA_ERR_DECODE: msg += " Decode error."; break;
                            case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED: msg += " Format not supported."; break;
                            default: msg += ` Code: ${mediaError.code}`;
                        }
                    }
                    setError(msg);
                }}
                onLoadedData={() => setError(null)}
            />

            {/* V1 Preview Disclaimer - visible to user */}
            <div className="video-overlay">
                <span className="video-path">
                    {activeClip?.source_file?.split(/[/\\]/).pop() ?? "Unknown"}
                </span>
                <span className="preview-disclaimer" style={{
                    fontSize: "0.65rem",
                    opacity: 0.6,
                    marginTop: "2px",
                    display: "block"
                }}>
                    Preview is approximate. Export is authoritative.
                </span>
            </div>

            <div className="video-controls">
                <button className="btn-play" onClick={togglePlay}>
                    {isPlaying ? "⏸" : "▶"}
                </button>
                <span className="time-display">
                    {playheadTime.toFixed(1)}s / {clips.reduce((max, c) => Math.max(max, c.start + c.duration), 0).toFixed(1)}s
                </span>
            </div>
        </div>
    );
}
