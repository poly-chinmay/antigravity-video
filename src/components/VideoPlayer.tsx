import { useEffect, useState, useRef } from "react";
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
}

export default function VideoPlayer({ clips }: VideoPlayerProps) {
    const [videoUrl, setVideoUrl] = useState<string>("");
    const [error, setError] = useState<string | null>(null);
    const videoRef = useRef<HTMLVideoElement>(null);

    // V1 Logic: Always play the first clip if available
    const activeClip = clips.length > 0 ? clips[0] : null;

    useEffect(() => {
        if (activeClip) {
            const src = activeClip.source_file;
            // Convert the local file path to a URL that the webview can load
            const url = convertFileSrc(src);
            console.log("🎥 [VideoPlayer] Loading video from:", src);
            console.log("🔗 [VideoPlayer] Converted URL:", url);
            setVideoUrl(url);
            setError(null);

            // Optional: Reset to start when clip changes
            if (videoRef.current) {
                videoRef.current.currentTime = 0;
            }
        } else {
            setVideoUrl("");
        }
    }, [activeClip?.source_file]); // Only reload if source file changes

    if (!activeClip) {
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

    if (error) {
        return (
            <div className="video-player-container error">
                <div className="error-content">
                    <p>Failed to load video</p>
                    <small>{error}</small>
                    <small style={{ marginTop: "10px", opacity: 0.7 }}>Path: {activeClip.source_file}</small>
                </div>
            </div>
        );
    }

    return (
        <div className="video-player-container">
            <video
                ref={videoRef}
                className="video-element"
                src={videoUrl}
                controls
                autoPlay={false}
                onError={(e) => {
                    const target = e.target as HTMLVideoElement;
                    const error = target.error;
                    console.error("Video Error Event:", e);
                    console.error("Media Error Details:", error);
                    let msg = "Playback failed.";
                    if (error) {
                        switch (error.code) {
                            case MediaError.MEDIA_ERR_ABORTED: msg += " Aborted."; break;
                            case MediaError.MEDIA_ERR_NETWORK: msg += " Network error."; break;
                            case MediaError.MEDIA_ERR_DECODE: msg += " Decode error."; break;
                            case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED: msg += " Format not supported."; break;
                            default: msg += ` Code: ${error.code}`;
                        }
                    }
                    setError(msg);
                }}
            />
            <div className="video-overlay">
                <span className="video-path">{activeClip.source_file.split(/[/\\]/).pop()}</span>
            </div>
        </div>
    );
}
