// Tauri commands for undo/redo operations
use crate::timeline;
use crate::timeline::TimelineEngine;
use crate::undo_redo_manager::UndoRedoManager;
use tauri::{Emitter, State};

#[tauri::command]
pub fn undo_command(
    engine: State<'_, TimelineEngine>,
    undo_manager: State<'_, std::sync::Mutex<UndoRedoManager>>,
    app_handle: tauri::AppHandle,
) -> Result<timeline::TimelineState, String> {
    let mut state = engine.state.lock().map_err(|_| "Failed to lock state")?;
    let mut manager = undo_manager
        .lock()
        .map_err(|_| "Failed to lock undo manager")?;

    manager.undo(&mut state)?;

    // Emit state update
    app_handle.emit("STATE_UPDATE", &*state).ok();

    Ok(state.clone())
}

#[tauri::command]
pub fn redo_command(
    engine: State<'_, TimelineEngine>,
    undo_manager: State<'_, std::sync::Mutex<UndoRedoManager>>,
    app_handle: tauri::AppHandle,
) -> Result<timeline::TimelineState, String> {
    let mut state = engine.state.lock().map_err(|_| "Failed to lock state")?;
    let mut manager = undo_manager
        .lock()
        .map_err(|_| "Failed to lock undo manager")?;

    manager.redo(&mut state)?;

    // Emit state update
    app_handle.emit("STATE_UPDATE", &*state).ok();

    Ok(state.clone())
}

#[tauri::command]
pub fn undo_multiple_command(
    count: usize,
    engine: State<'_, TimelineEngine>,
    undo_manager: State<'_, std::sync::Mutex<UndoRedoManager>>,
    app_handle: tauri::AppHandle,
) -> Result<timeline::TimelineState, String> {
    let mut state = engine.state.lock().map_err(|_| "Failed to lock state")?;
    let mut manager = undo_manager
        .lock()
        .map_err(|_| "Failed to lock undo manager")?;

    manager.undo_multiple(count, &mut state)?;

    // Emit state update
    app_handle.emit("STATE_UPDATE", &*state).ok();

    Ok(state.clone())
}

#[tauri::command]
pub fn can_undo(
    undo_manager: State<'_, std::sync::Mutex<UndoRedoManager>>,
) -> Result<bool, String> {
    let manager = undo_manager
        .lock()
        .map_err(|_| "Failed to lock undo manager")?;
    Ok(manager.can_undo())
}

#[tauri::command]
pub fn can_redo(
    undo_manager: State<'_, std::sync::Mutex<UndoRedoManager>>,
) -> Result<bool, String> {
    let manager = undo_manager
        .lock()
        .map_err(|_| "Failed to lock undo manager")?;
    Ok(manager.can_redo())
}
