//! Observable platform-route evidence for the non-canonical candidate.

use std::path::Path;
use std::process::Command;

use serde::Serialize;
use ulid::Ulid;

#[derive(Clone, Debug, Serialize)]
pub struct GraphicsProbe {
    pub api_version: String,
    pub renderer: String,
    pub shading_language_version: String,
}

#[derive(Serialize)]
struct Report {
    schema: &'static str,
    trace_id: String,
    evaluation_candidate: bool,
    canonical_platform_stack: bool,
    route: Route,
    observed_runtime: ObservedRuntime,
    boundaries: Boundaries,
}

#[derive(Serialize)]
struct Route {
    first_stage: &'static str,
    next_evaluation: &'static str,
    zeno_adoption: &'static str,
    portability_direction: &'static str,
    exhaustive_target_catalog: &'static str,
}

#[derive(Serialize)]
struct ObservedRuntime {
    os: &'static str,
    architecture: &'static str,
    mac_stage_observation: &'static str,
    graphics: GraphicsProbe,
    neovim_version: Option<String>,
}

#[derive(Serialize)]
struct Boundaries {
    graphics_api_choice: &'static str,
    host_language_choice: &'static str,
    root_ui_adoption: &'static str,
    aish_integration_mechanism: &'static str,
}

pub fn write_report(path: &Path, graphics: GraphicsProbe) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(&report(graphics))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(path, json)
}

fn report(graphics: GraphicsProbe) -> Report {
    Report {
        schema: "nvimgl.platform-route-evaluation/v1",
        trace_id: Ulid::new().to_string(),
        evaluation_candidate: true,
        canonical_platform_stack: false,
        route: Route {
            first_stage: "mac",
            next_evaluation: "zeno",
            zeno_adoption: "awaiting_human_gate",
            portability_direction: "multi_target",
            exhaustive_target_catalog: "unresolved",
        },
        observed_runtime: ObservedRuntime {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            mac_stage_observation: if cfg!(target_os = "macos") {
                "executed_on_mac"
            } else {
                "not_current_host"
            },
            graphics,
            neovim_version: neovim_version(),
        },
        boundaries: Boundaries {
            graphics_api_choice: "candidate_only_not_canonical",
            host_language_choice: "candidate_only_not_canonical",
            root_ui_adoption: "evaluation_hypothesis_only",
            aish_integration_mechanism: "replaceable_candidate_only",
        },
    }
}

fn neovim_version() -> Option<String> {
    let output = Command::new("nvim").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .next()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_keeps_the_multi_target_route_open() {
        let value = serde_json::to_value(report(GraphicsProbe {
            api_version: "test".to_owned(),
            renderer: "test".to_owned(),
            shading_language_version: "GLSL test".to_owned(),
        }))
        .unwrap();
        assert_eq!(value["route"]["first_stage"], "mac");
        assert_eq!(value["route"]["next_evaluation"], "zeno");
        assert_eq!(value["route"]["portability_direction"], "multi_target");
        assert_eq!(value["route"]["exhaustive_target_catalog"], "unresolved");
        assert_eq!(value["canonical_platform_stack"], false);
    }
}
