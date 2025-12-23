import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface MissionControlProps {
    onPreviewReady: (path: string) => void;
}

export default function MissionControl({ onPreviewReady }: MissionControlProps) {
    const [prompt, setPrompt] = useState("");
    const [status, setStatus] = useState<string | null>(null);
    const [isProcessing, setIsProcessing] = useState(false);
    const [isRendering, setIsRendering] = useState(false);

    async function runAiJsonEdit() {
        if (!prompt) return;
        setIsProcessing(true);
        setStatus("Processing request...");

        try {
            // Note: The backend command argument name must match exactly what is defined in Rust.
            // In lib.rs: process_user_prompt(..., user_input: String, ...)
            // But wait, apply_edit_plan signature is:
            // async fn apply_edit_plan(..., raw_llm_output: String)
            // Ah, the previous implementation used 'process_user_prompt' to get JSON, then 'apply_edit_plan'.
            // Let's check lib.rs to be sure which command to call.
            // Week 8 implementation: apply_edit_plan takes raw_llm_output.
            // But we want to send a PROMPT and have the backend do the rest?
            // Week 8 summary says: "Implemented the apply_edit_plan command, orchestrating the pipeline."
            // Let's check lib.rs in a moment. For now, I'll assume we need to call 'process_user_prompt' first?
            // Actually, let's look at the previous working code in App.tsx (step 467).
            // It called 'process_user_prompt' then 'apply_edit_plan'.
            // BUT, the user request for Week 12 implies a simpler flow or maybe I should stick to the robust one.
            // Let's stick to the robust flow: Prompt -> LLM -> Edit Plan -> Apply.
            // However, to keep it simple for this UI, maybe we just call 'process_user_prompt' and let it handle it?
            // No, 'process_user_prompt' returns LlmResponseMetadata.
            // We need to chain them.

            // Let's implement the chain here.

            // 1. Get LLM Response
            console.log("🚀 [Frontend] Sending prompt to backend:", prompt);
            const llmResponse = await invoke<any>("process_user_prompt", {
                userInput: prompt,
                requestId: crypto.randomUUID(), // Required by backend
                promptOverride: null
            });
            console.log("✅ [Frontend] Received LLM Response:", llmResponse);

            // 2. Apply the plan (if it's valid JSON)
            console.log("🚀 [Frontend] Sending raw output to apply_edit_plan:", llmResponse.text);
            const result = await invoke<string>("apply_edit_plan", {
                rawLlmOutput: llmResponse.text
            });
            console.log("✅ [Frontend] Apply Edit Plan Result:", result);

            setStatus(`Success: ${result}`);
        } catch (error) {
            console.error("AI Edit Error:", error);
            setStatus(`Error: ${error}`);
        } finally {
            setIsProcessing(false);
        }
    }

    async function handleRender() {
        setIsRendering(true);
        setStatus("Rendering preview...");
        try {
            const path = await invoke<string>("render_preview");
            setStatus("Render complete!");
            onPreviewReady(path);
        } catch (error) {
            console.error("Render Error:", error);
            setStatus(`Render Failed: ${error}`);
        } finally {
            setIsRendering(false);
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
                        onClick={handleRender}
                        disabled={isRendering || isProcessing}
                        style={{ flex: 1 }}
                    >
                        {isRendering ? "Rendering..." : "Render Preview"}
                    </button>
                </div>

                {status && (
                    <div className={`status-banner ${status.startsWith("Error") || status.startsWith("Render Failed") ? "status-error" : "status-success"}`}>
                        {status}
                    </div>
                )}
            </div>
        </div>
    );
}
