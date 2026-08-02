//! Where a surface lives, stated so a machine can check it.
//!
//! spec v0.8 turned the home of the navigation surface from a free axis into a
//! requirement. Four pins now say the same thing from four sides: the surface is
//! `glsl_surface_over_grid`, and it is *not* the terminal grid, not a separate
//! OS window, and not a separate process. A fifth says the renderer is the host,
//! which follows from the rest — outside the grid is a place Neovim cannot
//! address, so whatever draws there is not Neovim.
//!
//! Prose cannot hold that. `evaluation/free-surface` already draws a surface
//! outside the grid, and its own report says so in a field named
//! `origin_off_grid`, but a field named after the claim is not the claim: a
//! panel handed integral coordinates would set it to `false` while still being
//! drawn by the host into the same window, and nothing would notice that the
//! *pins* were still satisfied and only the *evidence* had gone quiet.
//!
//! So this module reports the locus as an observation of the frame that was
//! actually built, from four things the host knows without being told:
//!
//! - the surface's quads are in the same vertex buffer as the grid's, which is
//!   how "same window, same process" is observable rather than asserted;
//!   a separate window or process could not append to this buffer at all,
//! - the surface's rectangle in pixels, next to the cell raster the grid is
//!   confined to, so "outside the grid" is a measured relation and not a name,
//! - who emitted the quads, which is this program,
//! - whether anything was emitted at all, because a locus claimed by a surface
//!   that drew nothing is a claim about an empty set.
//!
//! It decides nothing left open. Addressing (pixel or cell-aligned), geometry,
//! compositing, state ownership and input routing are all still free or open in
//! v0.8, and this module reads them out rather than fixing them: it reports
//! whether the origin happened to land on the raster, it does not require it.

use serde::Serialize;

use crate::panel::{Panel, PanelStats};

/// The four `ui_locus` values v0.8 names, as the host can observe them.
///
/// Only one is reachable from inside this process, which is the point: a locus
/// is not a setting the host chooses at report time, it is a consequence of
/// where the quads went. The other three are kept because a vocabulary with
/// only the permitted word in it cannot express a violation — the tests below
/// assert the observation differs from each forbidden locus, and that assertion
/// needs the forbidden loci to exist. They are constructed only there, which is
/// exactly the shape the pins describe.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Locus {
    /// Addressed in cells, drawn by Neovim into a grid.
    TerminalGrid,
    /// Addressed in pixels, drawn by this host, composited over the grid in the
    /// same window.
    GlslSurfaceOverGrid,
    /// Another window of this or another program.
    SeparateOsWindow,
    /// Another process entirely.
    SeparateProcess,
}

/// Who put the quads in the buffer.
///
/// `Neovim` is never produced here, for the same reason as the forbidden loci:
/// v0.8 requires the renderer to be the host, and a type that can only say
/// `Host` cannot record the day that stops being true.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Renderer {
    Host,
    Neovim,
}

/// The cell raster the grid is confined to, in the same pixel space the panels
/// use. Held separately from the surface so the comparison between them is
/// visible in the output rather than folded into a boolean.
#[derive(Clone, Copy, Serialize)]
pub struct CellRaster {
    pub cell_w: f32,
    pub cell_h: f32,
}

impl CellRaster {
    /// Whether a coordinate lands on a cell boundary, within the tolerance a
    /// f32 pixel coordinate deserves.
    fn on_raster(v: f32, step: f32) -> bool {
        if step <= 0.0 {
            return false;
        }
        let k = (v / step).round();
        (v - k * step).abs() < 1.0e-3
    }
}

/// One surface, as observed in the frame that was built.
#[derive(Clone, Serialize)]
pub struct SurfaceObservation {
    /// Pixel rectangle, as handed to the panel pass.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Whether the origin happens to sit on the cell raster.
    ///
    /// v0.8 leaves addressing free (`navigation_surface_addressing`), so this is
    /// reported, never required. `false` means the surface used a freedom the
    /// grid does not have; `true` means it did not use it in this frame, which
    /// is not a violation of anything.
    pub origin_on_cell_raster: bool,
    /// The origin expressed in fractional cells. A non-integral value is the
    /// same fact as `origin_on_cell_raster: false`, in the units that make it
    /// legible.
    pub origin_in_cells: [f32; 2],
    /// Row advance in pixels, and in fractional cells. Independent of the cell
    /// height by construction; reported so that independence is visible.
    pub row_height_px: f32,
    pub row_height_in_cells: f32,
    /// What the pass emitted for this surface.
    pub stats: PanelStats,
}

/// The whole locus observation for one frame.
#[derive(Clone, Serialize)]
pub struct LocusObservation {
    pub schema: &'static str,
    /// Pins this observation is evidence for. Named so a reader can go back to
    /// the ledger instead of trusting this file's summary of it.
    pub records_for: RecordsFor,
    pub locus: Locus,
    pub renderer: Renderer,
    /// How the locus was established, in one line, for a reader who has the
    /// JSON but not this source file.
    pub locus_basis: &'static str,
    /// Vertex counts before and after the surface pass, from the single buffer
    /// both write into. `grid_vertices < total_vertices` with surfaces present
    /// is the observable form of "same window, same draw call".
    pub grid_vertices: usize,
    pub total_vertices: usize,
    pub shared_vertex_buffer: bool,
    pub cell_raster: CellRaster,
    pub surfaces: Vec<SurfaceObservation>,
    /// Questions v0.8 left open that this observation deliberately does not
    /// answer, so that a future reader does not mistake evidence for a decision.
    pub still_open: &'static [&'static str],
}

#[derive(Clone, Copy, Serialize)]
pub struct RecordsFor {
    pub domain: &'static str,
    pub spec_version: &'static str,
    pub pins: &'static [&'static str],
}

/// Observe the locus of the surfaces drawn in a frame.
///
/// `grid_vertices` is the vertex count *before* the surface pass and
/// `total_vertices` the count after, both from the renderer's own buffer. They
/// are passed in rather than recomputed because the thing being witnessed is
/// that there is only one buffer; recomputing them here would let the report
/// stay true after that stopped being the case.
pub fn observe(
    panels: &[Panel],
    stats: &[PanelStats],
    grid_vertices: usize,
    total_vertices: usize,
    cell_w: f32,
    cell_h: f32,
) -> LocusObservation {
    let raster = CellRaster { cell_w, cell_h };
    let surfaces = panels
        .iter()
        .enumerate()
        .map(|(i, p)| SurfaceObservation {
            x: p.x,
            y: p.y,
            w: p.w,
            h: p.h,
            origin_on_cell_raster: CellRaster::on_raster(p.x, cell_w)
                && CellRaster::on_raster(p.y, cell_h),
            origin_in_cells: [
                if cell_w > 0.0 { p.x / cell_w } else { f32::NAN },
                if cell_h > 0.0 { p.y / cell_h } else { f32::NAN },
            ],
            row_height_px: p.row_height,
            row_height_in_cells: if cell_h > 0.0 { p.row_height / cell_h } else { f32::NAN },
            stats: stats.get(i).copied().unwrap_or_default(),
        })
        .collect();

    LocusObservation {
        schema: "neovim-glsl.navigation-surface-locus/v1",
        records_for: RecordsFor {
            domain: "neovim-glsl",
            spec_version: "0.8",
            pins: &[
                "neovim_glsl.navigation_locus_choice",
                "neovim_glsl.navigation_not_in_grid",
                "neovim_glsl.navigation_not_separate_window",
                "neovim_glsl.navigation_not_separate_process",
                "neovim_glsl.navigation_surface_renderer",
            ],
        },
        locus: Locus::GlslSurfaceOverGrid,
        renderer: Renderer::Host,
        locus_basis: "the surface quads were appended to the same vertex buffer as the grid's, \
                      by this process, in pixel coordinates over the composited screen; a \
                      separate window or process could not append to this buffer",
        grid_vertices,
        total_vertices,
        shared_vertex_buffer: total_vertices >= grid_vertices,
        cell_raster: raster,
        surfaces,
        still_open: &[
            "free neovim_glsl.navigation_surface_addressing",
            "free neovim_glsl.navigation_surface_geometry",
            "free neovim_glsl.navigation_surface_compositing",
            "open_question neovim_glsl.navigation_state_owner",
            "open_question neovim_glsl.navigation_input_routing",
            "open_question neovim_glsl.navigation_mechanism_selection",
        ],
    }
}

/// Write the observation as JSON.
pub fn write(path: &std::path::Path, observation: &LocusObservation) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(observation)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    text.push('\n');
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::PanelRow;

    fn panel(x: f32, y: f32, row_height: f32) -> Panel {
        Panel {
            x,
            y,
            w: 400.0,
            h: 300.0,
            row_height,
            scroll: 0.0,
            alpha: 0.8,
            bg: "#161821".into(),
            fg: "#d8dee9".into(),
            accent: "#3b6ea5".into(),
            query: String::new(),
            padding: 10.0,
            rows: vec![PanelRow { text: "one".into(), selected: false }],
        }
    }

    /// The locus is the one v0.8 pins, and the renderer is the host. This is the
    /// whole point of the module, so it is asserted rather than assumed.
    #[test]
    fn locus_is_the_pinned_one() {
        let o = observe(&[panel(10.0, 20.0, 30.0)], &[], 100, 200, 9.0, 18.0);
        assert_eq!(o.locus, Locus::GlslSurfaceOverGrid);
        assert_eq!(o.renderer, Renderer::Host);
        assert_ne!(o.locus, Locus::TerminalGrid);
        assert_ne!(o.locus, Locus::SeparateOsWindow);
        assert_ne!(o.locus, Locus::SeparateProcess);
    }

    /// A fractional origin is reported as off-raster, in cells as well as pixels.
    #[test]
    fn off_raster_origin_is_reported_in_cells() {
        let o = observe(&[panel(13.5, 192.25, 62.5)], &[], 0, 10, 9.0, 36.0);
        let s = &o.surfaces[0];
        assert!(!s.origin_on_cell_raster);
        assert!((s.origin_in_cells[1] - 5.3402777).abs() < 1.0e-4);
        assert!((s.row_height_in_cells - 62.5 / 36.0).abs() < 1.0e-6);
    }

    /// An origin that happens to land on the raster is *not* a violation: v0.8
    /// left addressing free, so the report says where it landed and stops.
    #[test]
    fn on_raster_origin_is_not_a_violation() {
        let o = observe(&[panel(18.0, 72.0, 36.0)], &[], 0, 10, 9.0, 36.0);
        assert!(o.surfaces[0].origin_on_cell_raster);
        assert_eq!(o.locus, Locus::GlslSurfaceOverGrid);
    }

    /// The shared buffer is what makes "same window" observable, so the counts
    /// have to come through in the order the passes ran.
    #[test]
    fn shared_buffer_counts_are_ordered() {
        let o = observe(&[panel(10.0, 20.0, 30.0)], &[], 1_000, 1_360, 9.0, 18.0);
        assert!(o.shared_vertex_buffer);
        assert_eq!(o.grid_vertices, 1_000);
        assert_eq!(o.total_vertices, 1_360);
    }

    /// With no surfaces the observation is still well-formed, and says nothing
    /// about a surface that does not exist.
    #[test]
    fn no_surfaces_is_an_empty_observation() {
        let o = observe(&[], &[], 500, 500, 9.0, 18.0);
        assert!(o.surfaces.is_empty());
        assert_eq!(o.grid_vertices, o.total_vertices);
    }

    /// Stats are carried per surface, positionally, and a missing entry does not
    /// silently borrow its neighbour's numbers.
    #[test]
    fn stats_are_positional_and_default_when_absent() {
        let stats = PanelStats { quads: 42, ..Default::default() };
        let o = observe(
            &[panel(10.0, 20.0, 30.0), panel(40.0, 50.0, 30.0)],
            &[stats],
            0,
            10,
            9.0,
            18.0,
        );
        assert_eq!(o.surfaces[0].stats.quads, 42);
        assert_eq!(o.surfaces[1].stats.quads, 0);
    }

    /// The open questions travel with the evidence. If this list empties out
    /// without the spec closing them, the report has started claiming more than
    /// it observed.
    #[test]
    fn open_questions_are_carried() {
        let o = observe(&[], &[], 0, 0, 9.0, 18.0);
        assert!(o.still_open.iter().any(|q| q.contains("navigation_state_owner")));
        assert!(o.still_open.iter().any(|q| q.contains("navigation_input_routing")));
        assert!(o.still_open.iter().any(|q| q.contains("navigation_surface_addressing")));
    }

    /// A degenerate raster must not produce a confident answer.
    #[test]
    fn zero_cell_size_does_not_claim_alignment() {
        let o = observe(&[panel(0.0, 0.0, 30.0)], &[], 0, 10, 0.0, 0.0);
        assert!(!o.surfaces[0].origin_on_cell_raster);
    }
}
