import React from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';

interface PreviewPlayerProps {
    videoPath: string;
}

export const PreviewPlayer: React.FC<PreviewPlayerProps> = ({ videoPath }) => {
    // Convert local file path to a URL that the webview can access
    const assetUrl = convertFileSrc(videoPath);
    console.log("📺 PreviewPlayer: videoPath =", videoPath);
    console.log("🔗 PreviewPlayer: assetUrl =", assetUrl);

    return (
        <div className="card">
            <div className="card-header">Preview Monitor</div>
            <div className="preview-container">
                <video
                    controls
                    autoPlay
                    src={assetUrl}
                    style={{ width: '100%', height: '100%' }}
                >
                    Your browser does not support the video tag.
                </video>
            </div>
            <div style={{ marginTop: '8px', fontSize: '0.8rem', color: 'var(--text-secondary)' }}>
                Source: {videoPath}
            </div>
        </div>
    );
};
