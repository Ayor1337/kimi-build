//! Logo component — renders an animated spinning moon (月之暗面).
//!
//! The moon is drawn procedurally in braille (2×4 dots per cell, so dots are
//! visually ~square) instead of static art: the light direction rotates
//! around the disc, sweeping the terminator through the phases — full moon →
//! crescent → new moon → crescent → full. The unlit side stays visible as a
//! dim disc ("dark side of the moon").
//!
//! Hidden entirely on legacy Windows consoles: the U+2800 braille block is
//! not covered by the ConHost raster fonts and would render as tofu.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::render::color::blend_color;
use crate::theme::Theme;

/// Braille art footprint in terminal cells (cols × rows). Matches the
/// dimensions of the static art this replaced so surrounding layouts are
/// unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Art {
    cols: u16,
    rows: u16,
}

const LOGO: Art = Art { cols: 14, rows: 7 };
const LOGO_SMALL: Art = Art { cols: 11, rows: 5 };

/// Height at or above which the small logo is shown (below it, no logo).
const SMALL_LOGO_MIN_HEIGHT: u16 = 22;
/// Height at or above which the full logo is shown.
const FULL_LOGO_MIN_HEIGHT: u16 = 26;

fn pick_logo(window_height: u16) -> Option<Art> {
    pick_logo_for(window_height, logo_hidden())
}

/// Pure tier selection so tests can drive the legacy-console flag directly.
fn pick_logo_for(window_height: u16, hidden: bool) -> Option<Art> {
    if hidden || window_height < SMALL_LOGO_MIN_HEIGHT {
        None
    } else if window_height < FULL_LOGO_MIN_HEIGHT {
        Some(LOGO_SMALL)
    } else {
        Some(LOGO)
    }
}

/// The braille art has no ASCII stand-in; see the module doc.
fn logo_hidden() -> bool {
    crate::glyphs::is_legacy_windows_console()
}

/// Animation phase in seconds since the first render. Wall-clock based so the
/// spin speed is independent of the frame rate.
fn anim_phase_secs() -> f32 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// Redraw cadence in frames per second. The moon spin is slow, so a few fps
/// looks smooth while sparing the long-lived welcome screen from full-rate
/// repaints.
const SHIMMER_FPS: f32 = 12.0;

/// Seconds per full moon-phase cycle (one complete "spin" of the light around
/// the disc). Phase 0 is the full moon, 0.5 the new moon.
const PHASE_CYCLE_SECS: f32 = 8.0;

/// Current moon phase in `[0, 1)`.
fn moon_phase() -> f32 {
    (anim_phase_secs() / PHASE_CYCLE_SECS) % 1.0
}

/// Quantized animation frame for the current wall-clock phase. The welcome
/// screen redraws only when this advances, throttling the animation to
/// ~`SHIMMER_FPS` rather than the full event-loop tick rate. Pinned to 0 when
/// the logo is hidden.
pub fn shimmer_frame() -> u64 {
    if logo_hidden() {
        return 0;
    }
    (anim_phase_secs() * SHIMMER_FPS) as u64
}

/// Braille dot bit for (dx, dy) within a 2×4 cell (Unicode U+2800 + bits).
const BRAILLE_BITS: [[u32; 2]; 4] = [
    [0x01, 0x08], // dy = 0
    [0x02, 0x10], // dy = 1
    [0x04, 0x20], // dy = 2
    [0x40, 0x80], // dy = 3
];

/// One animation frame: the braille lines plus, per cell, the fraction of its
/// on-disc dots that are lit (drives the color ramp across the terminator).
fn moon_frame(art: Art, phase: f32) -> (Vec<String>, Vec<Vec<f32>>) {
    let cols = art.cols as usize;
    let rows = art.rows as usize;
    let w = cols as f32 * 2.0;
    let h = rows as f32 * 4.0;
    let cx = (w - 1.0) / 2.0;
    let cy = (h - 1.0) / 2.0;
    let r = w.min(h) / 2.0 - 1.0; // dot-space radius
    // Light direction rotates around the moon: φ=0 full, φ=π new.
    let phi = phase * std::f32::consts::TAU;
    let (sin_phi, cos_phi) = phi.sin_cos();

    let mut lit = vec![vec![0.0f32; cols]; rows];
    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut line = String::with_capacity(cols * 3);
        for col in 0..cols {
            let mut bits = 0u32;
            let mut n_lit = 0u32;
            for dy in 0..4 {
                for dx in 0..2 {
                    let px = (col * 2 + dx) as f32;
                    let py = (row * 4 + dy) as f32;
                    let nx = (px - cx) / r;
                    let ny = (py - cy) / r;
                    let d2 = nx * nx + ny * ny;
                    if d2 > 1.0 {
                        continue; // off the disc: blank braille
                    }
                    // On-disc dots are always drawn — the dark side stays
                    // visible as a dim disc ("dark side of the moon").
                    bits |= BRAILLE_BITS[dy][dx];
                    let z = (1.0 - d2).sqrt();
                    if nx * sin_phi + z * cos_phi > 0.0 {
                        n_lit += 1;
                    }
                }
            }
            lit[row][col] = n_lit as f32 / 8.0;
            line.push(char::from_u32(0x2800 + bits).unwrap_or('\u{2800}'));
        }
        lines.push(line);
    }
    (lines, lit)
}

fn render_into(area: Rect, buf: &mut Buffer, theme: &Theme, art: Art) {
    let (lines, lit) = moon_frame(art, moon_phase());

    // Color ramp: the lit face blends up to the bright text color, the dark
    // side rests at a dim gray just above the background; partial cells along
    // the terminator land in between.
    let hilite = theme.text_primary;
    let dim = blend_color(theme.gray, theme.bg_base, 0.55).unwrap_or(theme.gray);
    let logo_lines: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(row, line)| {
            let mut spans: Vec<Span> = Vec::new();
            let mut run = String::new();
            let mut run_color: Option<Color> = None;
            for (col, ch) in line.chars().enumerate() {
                let color =
                    blend_color(dim, hilite, lit[row][col].clamp(0.0, 1.0)).unwrap_or(dim);
                if run_color != Some(color) {
                    if let Some(prev) = run_color {
                        spans.push(Span::styled(
                            std::mem::take(&mut run),
                            Style::default().fg(prev),
                        ));
                    }
                    run_color = Some(color);
                }
                run.push(ch);
            }
            if let Some(prev) = run_color {
                spans.push(Span::styled(run, Style::default().fg(prev)));
            }
            Line::from(spans).alignment(Alignment::Center)
        })
        .collect();
    Paragraph::new(logo_lines).render(area, buf);
}

pub fn logo_line_count(window_height: u16) -> u16 {
    pick_logo(window_height).map_or(0, |art| art.rows)
}

pub fn logo_visual_width(window_height: u16) -> u16 {
    pick_logo(window_height).map_or(24, |art| art.cols)
}

pub fn render_logo(area: Rect, buf: &mut Buffer, theme: &Theme, window_height: u16) {
    if let Some(art) = pick_logo(window_height) {
        render_into(area, buf, theme, art);
    }
}

/// The hero box always shows the full logo: it is laid out beside the menu, so
/// it fits whenever the box does. These report and render that logo directly,
/// independent of the height-based [`pick_logo`] tiers used by the stacked
/// layout. When [`logo_hidden`], they report 0 and render nothing.
pub fn full_logo_line_count() -> u16 {
    full_logo_line_count_for(logo_hidden())
}

fn full_logo_line_count_for(hidden: bool) -> u16 {
    if hidden { 0 } else { LOGO.rows }
}

pub fn full_logo_visual_width() -> u16 {
    full_logo_visual_width_for(logo_hidden())
}

fn full_logo_visual_width_for(hidden: bool) -> u16 {
    if hidden { 0 } else { LOGO.cols }
}

pub fn render_full_logo(area: Rect, buf: &mut Buffer, theme: &Theme) {
    if !logo_hidden() {
        render_into(area, buf, theme, LOGO);
    }
}

/// Line count of the small logo used in minimal's committed welcome card
/// (0 on a legacy Windows console, where the braille art is suppressed).
pub fn compact_logo_line_count() -> u16 {
    if logo_hidden() { 0 } else { LOGO_SMALL.rows }
}

/// Render the small braille logo (centered) into `area` for minimal's welcome
/// card. No-op when the logo is hidden.
pub fn render_compact_logo(area: Rect, buf: &mut Buffer, theme: &Theme) {
    if !logo_hidden() {
        render_into(area, buf, theme, LOGO_SMALL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_sizes_by_height() {
        assert!(pick_logo_for(SMALL_LOGO_MIN_HEIGHT - 1, false).is_none());
        assert_eq!(
            pick_logo_for(SMALL_LOGO_MIN_HEIGHT, false),
            Some(LOGO_SMALL)
        );
        assert_eq!(
            pick_logo_for(FULL_LOGO_MIN_HEIGHT - 1, false),
            Some(LOGO_SMALL)
        );
        assert_eq!(pick_logo_for(FULL_LOGO_MIN_HEIGHT, false), Some(LOGO));
    }

    // The braille art has no legacy-safe stand-in, so every height tier must
    // collapse to no logo when the legacy-console flag is set.
    #[test]
    fn logo_hidden_on_legacy_console_at_every_height() {
        for h in [0, SMALL_LOGO_MIN_HEIGHT, FULL_LOGO_MIN_HEIGHT, u16::MAX] {
            assert!(pick_logo_for(h, true).is_none(), "height {h}");
        }
    }

    #[test]
    fn hero_box_always_uses_full_logo() {
        // The box renders the full logo regardless of height (it's laid out
        // beside the menu), and it's the large variant — never the small one.
        assert_eq!(full_logo_line_count_for(false), LOGO.rows);
        assert_eq!(full_logo_visual_width_for(false), LOGO.cols);
        assert!(full_logo_line_count_for(false) > LOGO_SMALL.rows);
        assert!(full_logo_visual_width_for(false) > LOGO_SMALL.cols);
    }

    #[test]
    fn full_logo_helpers_collapse_when_hidden() {
        assert_eq!(full_logo_line_count_for(true), 0);
        assert_eq!(full_logo_visual_width_for(true), 0);
    }

    #[test]
    fn compact_logo_line_count_matches_small_logo_when_visible() {
        // The minimal welcome card budgets exactly the small logo's rows. When
        // the logo isn't hidden, the count equals the small art's line count and
        // is strictly shorter than the full logo.
        if !logo_hidden() {
            assert_eq!(compact_logo_line_count(), LOGO_SMALL.rows);
            assert!(compact_logo_line_count() < LOGO.rows);
            assert!(compact_logo_line_count() > 0);
        } else {
            assert_eq!(compact_logo_line_count(), 0);
        }
    }

    #[test]
    fn moon_frame_dimensions_match_art() {
        for art in [LOGO, LOGO_SMALL] {
            let (lines, lit) = moon_frame(art, 0.25);
            assert_eq!(lines.len(), art.rows as usize);
            assert_eq!(lit.len(), art.rows as usize);
            for (line, lit_row) in lines.iter().zip(&lit) {
                assert_eq!(line.chars().count(), art.cols as usize);
                assert_eq!(lit_row.len(), art.cols as usize);
            }
        }
    }

    #[test]
    fn moon_full_phase_lights_the_disc() {
        let (_, lit) = moon_frame(LOGO, 0.0);
        let center = lit[LOGO.rows as usize / 2][LOGO.cols as usize / 2];
        assert!(center > 0.8, "full moon center should be lit, got {center}");
    }

    #[test]
    fn moon_new_phase_darkens_the_disc() {
        let (_, lit) = moon_frame(LOGO, 0.5);
        let max = lit
            .iter()
            .flatten()
            .copied()
            .fold(0.0f32, |acc, l| acc.max(l));
        assert!(max < 0.4, "new moon should be (mostly) dark, max lit {max}");
    }

    #[test]
    fn moon_quarter_phase_splits_the_disc() {
        let (_, lit) = moon_frame(LOGO, 0.25);
        let mid = LOGO.rows as usize / 2;
        let row = &lit[mid];
        let left: f32 = row[..LOGO.cols as usize / 3].iter().sum();
        let right: f32 = row[LOGO.cols as usize * 2 / 3..].iter().sum();
        assert!(
            right > left,
            "quarter moon should be brighter on the light side: left {left}, right {right}"
        );
    }

    #[test]
    fn moon_phase_advances_with_time() {
        let phase = moon_phase();
        assert!((0.0..1.0).contains(&phase));
    }
}
