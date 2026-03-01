import React, { useMemo, useCallback, useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import '../styles/timeline.css';

interface Clip {
    id: string;
    source_file: string;
    start: number;
    duration: number;
    track_id: string;
}

interface Track {
    id: string;
    name: string;
    index: number;
}

interface TimelineState {
    clips: Clip[];
    tracks: Track[];
    duration: number;
    playhead_time: number;
    version: number;
}

interface TimelineProps {
    timelineState: TimelineState | null;
    playheadTime: number;
    onSeek: (time: number) => void;
}

// Interaction mode
type InteractionMode = 'none' | 'move' | 'trim-start' | 'trim-end';

// Drag state for preview during clip interaction
interface DragState {
    mode: InteractionMode;
    clipId: string;
    originalStart: number;
    originalDuration: number;
    originalTrackId: string;
    dragStartX: number;
    dragStartY: number;
    currentOffsetX: number;
    currentOffsetY: number;
    targetTrackId: string;
}

// Helper to generate a consistent color from a string ID
function stringToColor(str: string): string {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
        hash = str.charCodeAt(i) + ((hash << 5) - hash);
    }
    const h = Math.abs(hash) % 360;
    return `hsl(${h}, 60%, 35%)`;
}

const PIXELS_PER_SECOND = 50; // Zoom level
const TRIM_HANDLE_WIDTH = 8; // Width of trim handle in pixels
const TRACK_HEIGHT = 60; // Height of each track in pixels

// Convert pixels to time
function pixelsToTime(deltaX: number): number {
    return deltaX / PIXELS_PER_SECOND;
}

const Timeline: React.FC<TimelineProps> = ({ timelineState, playheadTime, onSeek }) => {
    const [dragState, setDragState] = useState<DragState | null>(null);
    const [error, setError] = useState<string | null>(null);
    const containerRef = useRef<HTMLDivElement>(null);

    // Get sorted tracks
    const sortedTracks = useMemo(() => {
        if (!timelineState) return [];
        return [...timelineState.tracks].sort((a, b) => a.index - b.index);
    }, [timelineState]);

    // Memoize the total width calculation
    const totalWidth = useMemo(() => {
        if (!timelineState) return 0;
        return Math.max(timelineState.duration * PIXELS_PER_SECOND + 200, 1000);
    }, [timelineState]);

    // Get track index from Y position
    const getTrackIdFromY = useCallback((y: number): string | null => {
        if (!timelineState) return null;
        const trackIndex = Math.floor(y / TRACK_HEIGHT);
        if (trackIndex >= 0 && trackIndex < sortedTracks.length) {
            return sortedTracks[trackIndex].id;
        }
        return null;
    }, [sortedTracks, timelineState]);

    // Handle click on timeline to seek (only if not dragging)
    const handleTimelineClick = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
        if (dragState) return;

        const rect = e.currentTarget.getBoundingClientRect();
        const x = e.clientX - rect.left + e.currentTarget.scrollLeft;
        const time = x / PIXELS_PER_SECOND;

        const maxTime = timelineState?.duration || 0;
        const clampedTime = Math.max(0, Math.min(time, maxTime));
        onSeek(clampedTime);
    }, [timelineState?.duration, onSeek, dragState]);

    // Start drag for left trim
    const startLeftTrim = useCallback((e: React.MouseEvent, clip: Clip) => {
        e.stopPropagation();
        e.preventDefault();
        setDragState({
            mode: 'trim-start',
            clipId: clip.id,
            originalStart: clip.start,
            originalDuration: clip.duration,
            originalTrackId: clip.track_id,
            dragStartX: e.clientX,
            dragStartY: e.clientY,
            currentOffsetX: 0,
            currentOffsetY: 0,
            targetTrackId: clip.track_id,
        });
        setError(null);
    }, []);

    // Start drag for right trim
    const startRightTrim = useCallback((e: React.MouseEvent, clip: Clip) => {
        e.stopPropagation();
        e.preventDefault();
        setDragState({
            mode: 'trim-end',
            clipId: clip.id,
            originalStart: clip.start,
            originalDuration: clip.duration,
            originalTrackId: clip.track_id,
            dragStartX: e.clientX,
            dragStartY: e.clientY,
            currentOffsetX: 0,
            currentOffsetY: 0,
            targetTrackId: clip.track_id,
        });
        setError(null);
    }, []);

    // Start drag for move
    const startMove = useCallback((e: React.MouseEvent, clip: Clip) => {
        e.stopPropagation();
        e.preventDefault();
        setDragState({
            mode: 'move',
            clipId: clip.id,
            originalStart: clip.start,
            originalDuration: clip.duration,
            originalTrackId: clip.track_id,
            dragStartX: e.clientX,
            dragStartY: e.clientY,
            currentOffsetX: 0,
            currentOffsetY: 0,
            targetTrackId: clip.track_id,
        });
        setError(null);
    }, []);

    // Handle mouse move (global during drag)
    useEffect(() => {
        if (!dragState) return;

        const handleMouseMove = (e: MouseEvent) => {
            const deltaX = e.clientX - dragState.dragStartX;
            const deltaY = e.clientY - dragState.dragStartY;

            // Calculate target track during move
            let newTargetTrackId = dragState.originalTrackId;
            if (dragState.mode === 'move' && containerRef.current) {
                const rect = containerRef.current.getBoundingClientRect();
                const y = e.clientY - rect.top;
                const trackId = getTrackIdFromY(y);
                if (trackId) {
                    newTargetTrackId = trackId;
                }
            }

            setDragState(prev => prev ? {
                ...prev,
                currentOffsetX: deltaX,
                currentOffsetY: deltaY,
                targetTrackId: newTargetTrackId,
            } : null);
        };

        const handleMouseUp = async () => {
            if (!dragState) return;

            const deltaTime = pixelsToTime(dragState.currentOffsetX);

            // Only commit if moved significantly
            if (Math.abs(dragState.currentOffsetX) > 5 || dragState.targetTrackId !== dragState.originalTrackId) {
                try {
                    if (dragState.mode === 'move') {
                        const newStart = Math.max(0, dragState.originalStart + deltaTime);
                        const trackChanged = dragState.targetTrackId !== dragState.originalTrackId;
                        await invoke('move_clip', {
                            clipId: dragState.clipId,
                            newStartTime: newStart,
                            newTrackId: trackChanged ? dragState.targetTrackId : null,
                        });
                        console.log(`✅ Moved clip to ${newStart.toFixed(2)}s on track ${dragState.targetTrackId}`);
                    } else if (dragState.mode === 'trim-end') {
                        const newDuration = Math.max(0.1, dragState.originalDuration + deltaTime);
                        await invoke('trim_clip', {
                            clipId: dragState.clipId,
                            newDuration: newDuration,
                        });
                        console.log(`✅ Trimmed clip end to ${newDuration.toFixed(2)}s`);
                    } else if (dragState.mode === 'trim-start') {
                        const newStart = Math.max(0, dragState.originalStart + deltaTime);
                        await invoke('trim_clip_start', {
                            clipId: dragState.clipId,
                            newStartTime: newStart,
                        });
                        console.log(`✅ Trimmed clip start to ${newStart.toFixed(2)}s`);
                    }
                } catch (err) {
                    console.error('Interaction rejected:', err);
                    setError(String(err));
                    setTimeout(() => setError(null), 3000);
                }
            }

            setDragState(null);
        };

        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                setDragState(null);
            }
        };

        window.addEventListener('mousemove', handleMouseMove);
        window.addEventListener('mouseup', handleMouseUp);
        window.addEventListener('keydown', handleKeyDown);

        return () => {
            window.removeEventListener('mousemove', handleMouseMove);
            window.removeEventListener('mouseup', handleMouseUp);
            window.removeEventListener('keydown', handleKeyDown);
        };
    }, [dragState, getTrackIdFromY]);

    // Calculate preview position for dragged clip
    const getClipPosition = useCallback((clip: Clip): number => {
        if (dragState && dragState.clipId === clip.id) {
            const deltaTime = pixelsToTime(dragState.currentOffsetX);
            if (dragState.mode === 'move' || dragState.mode === 'trim-start') {
                return Math.max(0, clip.start + deltaTime) * PIXELS_PER_SECOND;
            }
        }
        return clip.start * PIXELS_PER_SECOND;
    }, [dragState]);

    // Calculate preview duration for trimmed clip
    const getClipWidth = useCallback((clip: Clip): number => {
        if (dragState && dragState.clipId === clip.id) {
            const deltaTime = pixelsToTime(dragState.currentOffsetX);
            if (dragState.mode === 'trim-end') {
                const newDuration = Math.max(0.1, clip.duration + deltaTime);
                return newDuration * PIXELS_PER_SECOND - 1;
            } else if (dragState.mode === 'trim-start') {
                const newDuration = Math.max(0.1, clip.duration - deltaTime);
                return newDuration * PIXELS_PER_SECOND - 1;
            }
        }
        return clip.duration * PIXELS_PER_SECOND - 1;
    }, [dragState]);

    // Get preview track index for dragged clip
    const getClipTrackIndex = useCallback((clip: Clip): number => {
        if (dragState && dragState.clipId === clip.id && dragState.mode === 'move') {
            const targetTrack = sortedTracks.find(t => t.id === dragState.targetTrackId);
            if (targetTrack) return targetTrack.index;
        }
        const track = sortedTracks.find(t => t.id === clip.track_id);
        return track ? track.index : 0;
    }, [dragState, sortedTracks]);

    // Get mode label
    const getModeLabel = (): string => {
        if (!dragState) return '';
        switch (dragState.mode) {
            case 'move': return dragState.targetTrackId !== dragState.originalTrackId
                ? `• Moving to ${sortedTracks.find(t => t.id === dragState.targetTrackId)?.name || 'track'}...`
                : '• Moving...';
            case 'trim-start': return '• Trimming left...';
            case 'trim-end': return '• Trimming right...';
            default: return '';
        }
    };

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
        <div className="timeline-container" ref={containerRef}>
            <div className="timeline-header">
                <span>Timeline</span>
                <span>
                    {sortedTracks.length} Tracks • {timelineState.clips.length} Clips • {timelineState.duration.toFixed(2)}s
                    {dragState && (
                        <span style={{ color: 'var(--accent-color)', marginLeft: '8px' }}>
                            {getModeLabel()}
                        </span>
                    )}
                </span>
            </div>

            {/* Error Toast */}
            {error && (
                <div style={{
                    position: 'absolute',
                    top: '50px',
                    left: '50%',
                    transform: 'translateX(-50%)',
                    backgroundColor: 'rgba(239, 68, 68, 0.9)',
                    color: 'white',
                    padding: '8px 16px',
                    borderRadius: '4px',
                    fontSize: '0.85rem',
                    zIndex: 100,
                }}>
                    {error}
                </div>
            )}

            <div className="timeline-scroll-area" onClick={handleTimelineClick}>
                <div
                    className="timeline-tracks"
                    style={{ width: `${totalWidth}px` }}
                >
                    {/* Ruler */}
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
                        <div className="playhead-line" style={{ height: `${sortedTracks.length * TRACK_HEIGHT + 30}px` }} />
                    </div>

                    {/* Tracks */}
                    {sortedTracks.map((track) => (
                        <div
                            key={track.id}
                            className="timeline-track"
                            style={{
                                height: `${TRACK_HEIGHT}px`,
                                position: 'relative',
                                borderBottom: '1px solid rgba(255,255,255,0.1)',
                            }}
                        >
                            {/* Track Label */}
                            <div style={{
                                position: 'absolute',
                                left: 0,
                                top: 0,
                                padding: '4px 8px',
                                fontSize: '0.7rem',
                                color: 'rgba(255,255,255,0.5)',
                                pointerEvents: 'none',
                            }}>
                                {track.name}
                            </div>

                            {/* Clips on this track */}
                            {timelineState.clips
                                .filter(clip => {
                                    // Show clip on original track unless being moved
                                    if (dragState?.clipId === clip.id && dragState.mode === 'move') {
                                        return dragState.targetTrackId === track.id;
                                    }
                                    return clip.track_id === track.id;
                                })
                                .map((clip) => {
                                    const isDragging = dragState?.clipId === clip.id;
                                    const position = getClipPosition(clip);
                                    const width = getClipWidth(clip);

                                    return (
                                        <div
                                            key={clip.id}
                                            className="timeline-clip"
                                            style={{
                                                position: 'absolute',
                                                top: '4px',
                                                left: `${position}px`,
                                                width: `${width}px`,
                                                height: `${TRACK_HEIGHT - 8}px`,
                                                backgroundColor: stringToColor(clip.id),
                                                opacity: isDragging ? 0.8 : 1,
                                                boxShadow: isDragging
                                                    ? '0 4px 12px rgba(0,0,0,0.3)'
                                                    : 'none',
                                                zIndex: isDragging ? 10 : 1,
                                                transition: isDragging ? 'none' : 'box-shadow 0.15s',
                                                borderRadius: '4px',
                                                overflow: 'hidden',
                                            }}
                                            title={`ID: ${clip.id}\nTrack: ${track.name}\nStart: ${clip.start.toFixed(2)}s\nDur: ${clip.duration.toFixed(2)}s`}
                                        >
                                            {/* Left Trim Handle */}
                                            <div
                                                onMouseDown={(e) => startLeftTrim(e, clip)}
                                                style={{
                                                    position: 'absolute',
                                                    left: 0,
                                                    top: 0,
                                                    bottom: 0,
                                                    width: `${TRIM_HANDLE_WIDTH}px`,
                                                    backgroundColor: isDragging && dragState?.mode === 'trim-start'
                                                        ? 'rgba(255,255,255,0.4)'
                                                        : 'rgba(255,255,255,0.15)',
                                                    cursor: 'ew-resize',
                                                    borderRight: '1px solid rgba(255,255,255,0.2)',
                                                    zIndex: 2,
                                                }}
                                            />

                                            {/* Clip Body (for move) */}
                                            <div
                                                onMouseDown={(e) => startMove(e, clip)}
                                                style={{
                                                    position: 'absolute',
                                                    left: `${TRIM_HANDLE_WIDTH}px`,
                                                    right: `${TRIM_HANDLE_WIDTH}px`,
                                                    top: 0,
                                                    bottom: 0,
                                                    cursor: isDragging && dragState?.mode === 'move' ? 'grabbing' : 'grab',
                                                    padding: '4px',
                                                    overflow: 'hidden',
                                                }}
                                            >
                                                <div style={{
                                                    fontWeight: 'bold',
                                                    fontSize: '0.75rem',
                                                    overflow: 'hidden',
                                                    textOverflow: 'ellipsis',
                                                    whiteSpace: 'nowrap',
                                                }}>
                                                    {clip.source_file.split(/[/\\]/).pop()}
                                                </div>
                                                <div style={{ fontSize: '0.65rem', opacity: 0.8 }}>
                                                    {clip.duration.toFixed(1)}s
                                                </div>
                                            </div>

                                            {/* Right Trim Handle */}
                                            <div
                                                onMouseDown={(e) => startRightTrim(e, clip)}
                                                style={{
                                                    position: 'absolute',
                                                    right: 0,
                                                    top: 0,
                                                    bottom: 0,
                                                    width: `${TRIM_HANDLE_WIDTH}px`,
                                                    backgroundColor: isDragging && dragState?.mode === 'trim-end'
                                                        ? 'rgba(255,255,255,0.4)'
                                                        : 'rgba(255,255,255,0.15)',
                                                    cursor: 'ew-resize',
                                                    borderLeft: '1px solid rgba(255,255,255,0.2)',
                                                    zIndex: 2,
                                                }}
                                            />
                                        </div>
                                    );
                                })}
                        </div>
                    ))}
                </div>
            </div>
        </div>
    );
};

export default Timeline;
