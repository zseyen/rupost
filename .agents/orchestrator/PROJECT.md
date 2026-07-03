# Project: RuPost TUI Feature Development

## Architecture
- `src/tui/state.rs`: Stores TUI state including layout mode, file list, active panel, text area states, response body, etc.
- `src/tui/app.rs`: Main event loop handling key presses, background Tokio async request executor spawning, and UI redraw triggers.
- `src/tui/ui/mod.rs`: Renders the layout depending on terminal size (Wide, Narrow, Stacked), modal boxes (Help, Unsaved changes).
- `src/history/model.rs` & `recorder.rs`: Backward-compatible response history schema containing optional response body.

## Code Layout
- `src/tui/mod.rs` - Entry point
- `src/tui/event.rs` - Key actions and TUI events
- `src/tui/state.rs` - AppState logic and UI variables
- `src/tui/app.rs` - Async tokio event loop and terminal wrapper
- `src/tui/ui/mod.rs` - Screen widgets rendering

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | Explore & Architecture | Research current codebase structure and check if any TUI code exits | none | DONE |
| 2 | R3: History Compat | Ensure ResponseMeta and recorder support body option and backward compatibility | M1 | DONE |
| 3 | R1: Adaptive Layout & Focus | Implement Wide/Narrow/Stacked layouts, Tab/Shift+Tab focus, terminal too small center warning | M2 | IN_PROGRESS |
| 4 | R2: Request Run & Details | Asynchronous execution, loading state, show status/headers/body, response scroll | M3 | IN_PROGRESS |
| 5 | R4: Unsaved Changes Alert | Track dirty state, show y/n modal popup on q or file change | M4 | PLANNED |
| 6 | A1-A4: Verification | Run all tests (smoke tests, regressions) to verify code compilation and correctness | M5 | PLANNED |

## Interface Contracts
### `app` ↔ `state`
- `state.update_layout(width, height)`: Recalculate adaptive layout mode.
- `state.update(Action)`: State transitions via Action.
- `state.handle_request_finished(result, captured_vars, assertions)`: Handle completed HTTP requests non-blockingly.
