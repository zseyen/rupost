---
name: cli-aesthetic-ux
description: "Guidelines for maintaining Rupost's high-aesthetic terminal experience, HSL color palettes, and responsive TUI design."
---

# CLI & TUI Aesthetic Design Standards

## 1. Color Theory: The HSL Advantage
Avoid generic terminal colors. Use refined HSL (Hue, Saturation, Lightness) palettes for a premium feel.

- **Primary Brand Color**: Vibrant but not piercing (e.g., Hue around 210 for Blue or 280 for Purple).
- **Harmony**: Use complementary colors for Success (Greenish) and Error (Reddish) that match the primary hue's saturation level.
- **Contrast**: Ensure high readability by adjusting Lightness carefully for both dark and light terminal backgrounds.

## 2. Interaction & Feedback
The user should feel the tool is "alive" and responsive.

- **Micro-Animations**: Use smooth spinners (`indicatif` or custom) during long-running tasks.
- **Progressive Disclosure**: 
    - By default, show concise success messages.
    - Automatically show detailed failure logs if a request fails, avoiding the need for the user to manually add `--verbose` every time.
- **Contextual Help**: Follow the "Discoverability" principle. Key shortcuts (e.g., `a` for Analyze, `r` for Retry) should be visible or easily accessible in the TUI footer.

## 3. Responsive TUI Architecture
The UI MUST NEVER block on I/O.

- **Async decouple**: Use `tokio::sync::mpsc` channels to communicate between the UI thread and background workers.
- **Streaming**: For AI responses or large response bodies, implement partial rendering (Streaming) so the user sees progress immediately.
- **Input protection**: Always debounced or rate-limit input handling to prevent UI flicker.

## 4. Visual Layout
- **Minimalism**: "Less is more". Avoid borders where whitespace can suffice.
- **Consistency**: Use consistent padding and alignment across all TUI screens.
- **Metadata Visibility**: Important metadata like Response Time and Status Code should be prominent but not overwhelming.
