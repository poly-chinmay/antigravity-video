import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

/**
 * MediaPoolItem - Single imported media item
 */
interface MediaPoolItem {
    source_id: string;
    file_name: string;
    file_path: string;
    duration_secs: number;
    resolution: [number, number] | null;
    frame_rate: number | null;
    codec: string;
    file_size: number;
    status: 'Available' | 'Offline';
}

/**
 * MediaPoolViewModel - Complete pool state from backend
 */
interface MediaPoolViewModel {
    items: MediaPoolItem[];
    count: number;
    offline_count: number;
}

interface MediaPoolPanelProps {
    onClipAdded?: (clipId: string, fileName: string) => void;
}

/**
 * MediaPoolPanel - Displays imported media and allows adding to timeline
 * 
 * Pool-first workflow:
 * 1. Import adds to pool (not timeline)
 * 2. User explicitly adds from pool to timeline
 * 3. Same media can be reused multiple times
 */
export default function MediaPoolPanel({ onClipAdded }: MediaPoolPanelProps) {
    const [pool, setPool] = useState<MediaPoolViewModel | null>(null);
    const [loading, setLoading] = useState(true);
    const [addingId, setAddingId] = useState<string | null>(null);
    const [feedback, setFeedback] = useState<string | null>(null);

    // Fetch pool state
    const fetchPool = useCallback(async () => {
        try {
            const result = await invoke<MediaPoolViewModel>('get_media_pool');
            setPool(result);
        } catch (error) {
            console.error('Failed to fetch media pool:', error);
        } finally {
            setLoading(false);
        }
    }, []);

    // Initial fetch + refresh after import
    useEffect(() => {
        fetchPool();
        // Refresh pool every 2 seconds to catch new imports
        const interval = setInterval(fetchPool, 2000);
        return () => clearInterval(interval);
    }, [fetchPool]);

    // Add media to timeline
    const handleAddToTimeline = async (item: MediaPoolItem) => {
        // Block if offline
        if (item.status === 'Offline') {
            setFeedback(`❌ Cannot add: ${item.file_name} is offline`);
            setTimeout(() => setFeedback(null), 3000);
            return;
        }

        setAddingId(item.source_id);
        try {
            const clipId = await invoke<string>('add_media_to_timeline', {
                sourceId: item.source_id,
            });

            // Success feedback
            setFeedback(`✅ Added "${item.file_name}" to timeline`);
            onClipAdded?.(clipId, item.file_name);

            // Clear feedback after 3s
            setTimeout(() => setFeedback(null), 3000);
        } catch (error) {
            console.error('Failed to add to timeline:', error);
            setFeedback(`❌ Failed: ${error}`);
            setTimeout(() => setFeedback(null), 3000);
        } finally {
            setAddingId(null);
        }
    };

    // Format duration as MM:SS
    const formatDuration = (secs: number): string => {
        const mins = Math.floor(secs / 60);
        const remainingSecs = Math.floor(secs % 60);
        return `${mins}:${remainingSecs.toString().padStart(2, '0')}`;
    };

    if (loading) {
        return (
            <div className="card">
                <div className="card-header">Media Pool</div>
                <div style={{ padding: '12px', color: 'var(--text-secondary)' }}>
                    Loading...
                </div>
            </div>
        );
    }

    return (
        <div className="card">
            <div className="card-header">
                Media Pool
                <span style={{
                    fontSize: '0.75rem',
                    color: 'var(--text-secondary)',
                    marginLeft: '8px'
                }}>
                    {pool?.count || 0} items
                    {pool && pool.offline_count > 0 && (
                        <span style={{ color: '#f59e0b' }}>
                            {' '}({pool.offline_count} offline)
                        </span>
                    )}
                </span>
            </div>

            {/* Feedback Toast */}
            {feedback && (
                <div style={{
                    padding: '8px 12px',
                    backgroundColor: feedback.startsWith('✅') ? 'rgba(34, 197, 94, 0.2)' : 'rgba(239, 68, 68, 0.2)',
                    borderBottom: '1px solid var(--border-color)',
                    fontSize: '0.85rem',
                }}>
                    {feedback}
                </div>
            )}

            {/* Empty State */}
            {(!pool || pool.items.length === 0) && (
                <div style={{
                    padding: '20px 12px',
                    color: 'var(--text-secondary)',
                    textAlign: 'center',
                    fontSize: '0.9rem'
                }}>
                    No media imported yet.
                    <br />
                    <span style={{ fontSize: '0.8rem', opacity: 0.7 }}>
                        Use "Import Video" to add media.
                    </span>
                </div>
            )}

            {/* Media List */}
            <div style={{ maxHeight: '300px', overflowY: 'auto' }}>
                {pool?.items.map((item) => (
                    <div
                        key={item.source_id}
                        style={{
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            padding: '10px 12px',
                            borderBottom: '1px solid var(--border-color)',
                            opacity: item.status === 'Offline' ? 0.5 : 1,
                        }}
                    >
                        {/* Media Info */}
                        <div style={{ flex: 1, minWidth: 0 }}>
                            <div style={{
                                fontWeight: 500,
                                fontSize: '0.9rem',
                                overflow: 'hidden',
                                textOverflow: 'ellipsis',
                                whiteSpace: 'nowrap',
                            }}>
                                {item.file_name}
                            </div>
                            <div style={{
                                fontSize: '0.75rem',
                                color: 'var(--text-secondary)',
                                marginTop: '2px',
                            }}>
                                {formatDuration(item.duration_secs)}
                                {item.resolution && ` • ${item.resolution[0]}×${item.resolution[1]}`}
                                {item.status === 'Offline' && (
                                    <span style={{ color: '#f59e0b', marginLeft: '8px' }}>
                                        OFFLINE
                                    </span>
                                )}
                            </div>
                        </div>

                        {/* Add Button */}
                        <button
                            onClick={() => handleAddToTimeline(item)}
                            disabled={addingId === item.source_id || item.status === 'Offline'}
                            style={{
                                padding: '6px 12px',
                                fontSize: '0.8rem',
                                backgroundColor: item.status === 'Offline'
                                    ? 'var(--bg-secondary)'
                                    : 'var(--accent-color)',
                                color: item.status === 'Offline'
                                    ? 'var(--text-secondary)'
                                    : 'white',
                                border: 'none',
                                borderRadius: '4px',
                                cursor: item.status === 'Offline' ? 'not-allowed' : 'pointer',
                                whiteSpace: 'nowrap',
                            }}
                        >
                            {addingId === item.source_id ? '...' : '+ Add'}
                        </button>
                    </div>
                ))}
            </div>

            {/* Session Notice */}
            <div style={{
                padding: '8px 12px',
                fontSize: '0.7rem',
                color: 'var(--text-secondary)',
                borderTop: '1px solid var(--border-color)',
                textAlign: 'center',
            }}>
                Session-scoped • Media clears on restart
            </div>
        </div>
    );
}
