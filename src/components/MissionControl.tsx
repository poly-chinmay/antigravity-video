import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * MissionControl - AI Command Interface
 * 
 * This component handles AI-powered editing commands.
 * Export functionality is available via the Export button.
 */
export default function MissionControl() {
    const [prompt, setPrompt] = useState("");
    const [status, setStatus] = useState<string | null>(null);
    const [isProcessing, setIsProcessing] = useState(false);
    const [isExporting, setIsExporting] = useState(false);

    // STEP 4 FIX: Single atomic command - no more two-step process
    async function runAiJsonEdit() {
        if (!prompt) return;
        setIsProcessing(true);
        setStatus("Processing AI edit...");

        try {
            // Single atomic command: user intent → result
            // Backend handles: prompt → LLM → parse → validate → execute
            console.log("🚀 [Frontend] Sending to execute_ai_edit:", prompt);
            const result = await invoke<string>("execute_ai_edit", {
                userInput: prompt,
                requestId: crypto.randomUUID(),
            });
            console.log("✅ [Frontend] AI Edit Result:", result);
            setStatus(`Success: ${result}`);
        } catch (error) {
            console.error("AI Edit Error:", error);
            setStatus(`Error: ${error}`);
        } finally {
            setIsProcessing(false);
        }
    }

    async function handleExport() {
        setIsExporting(true);
        setStatus("Exporting timeline...");
        try {
            const path = await invoke<string>("export_timeline");
            setStatus(`Export complete: ${path.split('/').pop()}`);
            console.log("✅ [Frontend] Export saved to:", path);
        } catch (error) {
            console.error("Export Error:", error);
            setStatus(`Export Failed: ${error}`);
        } finally {
            setIsExporting(false);
        }
    }

    return (
        <div className="card">
            <div className="card-header">Mission Control</div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                <div>
                    <label style={{ display: 'block', marginBottom: '8px', fontSize: '0.9rem', color: 'var(--text-secondary)' }}>
                        AI Instructions
                    </label>
                    <textarea
                        className="input-field"
                        rows={4}
                        placeholder="e.g., 'Delete the first clip' or 'Trim the last clip by 2 seconds'"
                        value={prompt}
                        onChange={(e) => setPrompt(e.target.value)}
                        style={{ resize: 'vertical' }}
                    />
                </div>

                <div style={{ display: 'flex', gap: '10px' }}>
                    <button
                        className="btn-primary"
                        onClick={runAiJsonEdit}
                        disabled={isProcessing || !prompt}
                        style={{ flex: 1 }}
                    >
                        {isProcessing ? "Processing..." : "Run AI Edit"}
                    </button>

                    <button
                        className="btn-secondary"
                        onClick={handleExport}
                        disabled={isExporting || isProcessing}
                        style={{ flex: 1 }}
                    >
                        {isExporting ? "Exporting..." : "📦 Export"}
                    </button>
                </div>

                {status && (
                    <div className={`status-banner ${status.startsWith("Error") || status.startsWith("Export Failed") ? "status-error" : "status-success"}`}>
                        {status}
                    </div>
                )}
            </div>
        </div>
    );
}
