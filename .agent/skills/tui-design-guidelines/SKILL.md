---
name: tui-design-guidelines
description: Guides on building beautiful, responsive, and robust terminal user interfaces (TUI) with Clean Architecture and comprehensive off-screen terminal testing.
---

# TUI Design & Component Guidelines

## 1. TUI Component Design Philosophy

We adhere to **Clean Architecture** and unidirectional data flow when structuring terminal interfaces:
*   **Data vs. Render Separation (M-V-VM)**: UI widgets must remain purely functional and read-only. `AppState` preserves the logical model, while UI components (e.g., `sidebar::render`, `editor::render`) project the state onto the canvas. Never keep local mutable states inside UI render functions.
*   **Defensive Adaptive Layouts**: Avoid hardcoded width or height spans. Always use proportional calculations or dynamic constraints (e.g., Ratatui `Constraint::Percentage` or `Constraint::Min`). In extremely small screen dimensions, downgrade layout blocks gracefully to stacked formats or a generic warning screen instead of panicking.
*   **Action-driven Events**: State mutation is triggered exclusively by key/mouse events translated into Actions or direct TuiEvents processed in the main event thread, preserving the predictability of state changes.

## 2. Interface Aesthetics & Key Overload Constraints

*   **No Emoji Requirement**: Keep terminal interfaces professional and highly portable by excluding emoji characters from status blocks, lists, and dialogs. Use neat retro-ASCII indicators:
    *   `[RUNNING]`, `[SUCCESS]`, `[FAILED]` for task states.
    *   `*` for modified/unsaved editor states.
    *   `▸` (collapsed) and `▾` (expanded) for folder paths.
*   **Overloaded Key Navigation**: When left/right arrows or `h/j/k/l` keys represent both panel tabs shifting and directory expanding/collapsing:
    *   `Right / l` -> Expands selected directory if it is collapsed. If it is already expanded (or it's a file), route to tab selection.
    *   `Left / h` -> Collapses selected directory if it is expanded. Otherwise, route to tab selection.

## 3. Robust Engineering & Performance Safeguards

*   **Heuristic File Scan Guard (I/O & UTF-8 Protection)**: When scanning a workspace for files, never perform an unbounded load of files.
    *   Only read the first **1024 bytes** of candidate files to verify heuristically if they contain HTTP requests or Markdown rest-blocks.
    *   Always use UTF-8 lossy decoding (`String::from_utf8_lossy`) to prevent panics when reading binary files (e.g. from `.git/` or compiled targets) that bypass path filters.
*   **Long Response Viewport Truncation**: Rendering thousands of lines inside terminal widgets degrades performance. Truncate response logs and payloads above a safe boundary (e.g., 1000 lines) and display a clear performance banner warning the user, ensuring the UI thread remains highly interactive.
*   **Unsaved Dialog Decoupling for Read-Only views**: If a view is read-only (such as file previews or history replays), bypass `is_dirty` checks during file switches or exiting to prevent unsaved changes confirm modals from deadlocking user interaction.
*   **Incremental Word-Wrap Cache on Streams**: Do not compute text-wrapping inside the rendering cycle for live streams (like SSE or WebSockets). Compute wrapping incrementally as chunks arrive and cache the visual lines. On terminal resize, invalidate the cache lazily to prevent synchronous compute freezes.

## 4. Directory Tree Projection & Heuristics

*   **Heuristic is_dir for Mock Paths**: Since mock tests use virtual paths that do not exist physically on disk (where `Path::is_dir()` returns `false`), apply heuristics:
    *   All intermediate (non-leaf) path components are directories.
    *   Leaf components are files if they match specific HTTP/markdown extensions (`.http`, `.md`), otherwise they default to directories.
*   **Cursor Centripetal Rebound ("向心回弹")**: When a parent folder is collapsed, if the active cursor selection was inside that collapsed folder (meaning the selected index is now hidden), the selection index must immediately retract up the folder path to the parent directory to prevent index out-of-bounds panics.

## 5. Off-Screen TUI Functional Verification

*   **Off-screen TestBackend Simulation**: Always write smoke tests using `ratatui::backend::TestBackend` to simulate TUI operations:
    *   Initialize terminal canvas at multiple resolutions (e.g., `80x24` and extreme bounds like `35x5`) to check if any rendering cell out-of-bound panic occurs.
    *   Queue input keystrokes using crossterm events (e.g., Up/Down arrows, Enter) and call key handlers to verify that state values transition correctly.
*   **Log & Cache Resize Testing**: Simulate terminal resize events (`TuiEvent::Resize`) during active operations (like streaming SSE logs) to assert that visual line caches are invalidated and lazily computed without CPU blockages.
