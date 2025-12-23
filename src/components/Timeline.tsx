import React, { useMemo, useCallback } from 'react';
import '../styles/timeline.css';

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

interface TimelineProps {
    timelineState: TimelineState | null;
    playheadTime: number;
    onSeek: (time: number) => void;
}

// Helper to generate a consistent color from a string ID
function stringToColor(str: string): string {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
        hash = str.charCodeAt(i) + ((hash << 5) - hash);
    }
    // Generate HSL color for better aesthetics (darker tones)
    const h = Math.abs(hash) % 360;
    return `hsl(${h}, 60%, 35%)`; // Saturation 60%, Lightness 35%
}

const PIXELS_PER_SECOND = 50; // Zoom level

const Timeline: React.FC<TimelineProps> = ({ timelineState, playheadTime, onSeek }) => {
    // Memoize the total width calculation
    const totalWidth = useMemo(() => {
        if (!timelineState) return 0;
        // Add some padding at the end
        return Math.max(timelineState.duration * PIXELS_PER_SECOND + 200, 1000);
    }, [timelineState]);

    // Handle click on timeline to seek
    const handleTimelineClick = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const x = e.clientX - rect.left + e.currentTarget.scrollLeft;
        const time = x / PIXELS_PER_SECOND;

        // Clamp to valid range
        const maxTime = timelineState?.duration || 0;
        const clampedTime = Math.max(0, Math.min(time, maxTime));

        onSeek(clampedTime);
    }, [timelineState?.duration, onSeek]);

    if (!timelineState) {
        return (
            <div className="timeline-container">
                <div className="timeline-header">Timeline</div>
                <div style={{ padding: '20px', color: '#888', textAlign: 'center' }}>
                    Loading Timeline...
                </div>
            </div>
        );
    }

    return (
        <div className="timeline-container">
            <div className="timeline-header">
                <span>Timeline</span>
                <span>{timelineState.clips.length} Clips • {timelineState.duration.toFixed(2)}s</span>
            </div>

            <div className="timeline-scroll-area" onClick={handleTimelineClick}>
                <div
                    className="timeline-tracks"
                    style={{ width: `${totalWidth}px` }}
                >
                    {/* Ruler (Simple visualization) */}
                    <div className="timeline-ruler">
                        {Array.from({ length: Math.ceil(timelineState.duration + 5) }).map((_, i) => (
                            <div
                                key={i}
                                className="ruler-tick"
                                style={{ left: `${i * PIXELS_PER_SECOND}px` }}
                            >
                                {i}s
                            </div>
                        ))}
                    </div>

                    {/* Playhead Cursor */}
                    <div
                        className="timeline-playhead"
                        style={{ left: `${playheadTime * PIXELS_PER_SECOND}px` }}
                    >
                        <div className="playhead-handle" />
                        <div className="playhead-line" />
                    </div>

                    {/* Track 0 (Default for now) */}
                    <div className="timeline-track">
                        {timelineState.clips.map((clip) => (
                            <div
                                key={clip.id}
                                className="timeline-clip"
                                style={{
                                    left: `${clip.start * PIXELS_PER_SECOND}px`,
                                    width: `${clip.duration * PIXELS_PER_SECOND - 1}px`, // -1 for gap
                                    backgroundColor: stringToColor(clip.id)
                                }}
                                title={`ID: ${clip.id}\nSource: ${clip.source_file}\nStart: ${clip.start.toFixed(2)}s\nDur: ${clip.duration.toFixed(2)}s`}
                            >
                                <div style={{ fontWeight: 'bold', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                                    {clip.source_file.split(/[/\\]/).pop()}
                                </div>
                                <div style={{ fontSize: '0.7rem', opacity: 0.8 }}>
                                    {clip.duration.toFixed(1)}s
                                </div>
                            </div>
                        ))}
                    </div>
                </div>
            </div>
        </div>
    );
};

export default Timeline;
