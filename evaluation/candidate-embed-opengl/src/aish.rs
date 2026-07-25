//! Replaceable, read-only aish bridge for this evaluation candidate.
//!
//! This commences the required aish integration without choosing a canonical
//! transport or exposing execution. In particular, there is deliberately no
//! bridge command for `aish run` or `aish exec`: the effect-confirmation UI is
//! still an open question in the domain spec.

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use rmpv::Value as RpcValue;
use serde::Serialize;
use serde_json::{json, Value};
use ulid::Ulid;

const SCHEMA: &str = "nvimgl.aish-readonly-bridge/v1";
const MAX_DIAGNOSTIC_CHARS: usize = 2_048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Discover,
    Status,
    Inspect { kind: String, identity: String },
}

impl Request {
    pub fn from_rpc(params: &[RpcValue]) -> Result<Self, ErrorEnvelope> {
        match params.first().and_then(RpcValue::as_str) {
            Some("discover") => Ok(Self::Discover),
            Some("status") => Ok(Self::Status),
            Some("inspect") => {
                let kind = params.get(1).and_then(RpcValue::as_str).ok_or_else(|| {
                    error("invalid_request", "AishInspect requires an object kind.")
                })?;
                let identity = params
                    .get(2)
                    .and_then(RpcValue::as_str)
                    .ok_or_else(|| error("invalid_request", "AishInspect requires an identity."))?;
                if !matches!(
                    kind,
                    "file" | "process" | "port" | "service" | "log" | "executable" | "repository"
                ) {
                    return Err(error(
                        "unsupported_object_kind",
                        "AishInspect received an unsupported object kind.",
                    ));
                }
                Ok(Self::Inspect {
                    kind: kind.to_owned(),
                    identity: identity.to_owned(),
                })
            }
            _ => Err(error(
                "unsupported_request",
                "The read-only aish bridge supports discover, status, and inspect.",
            )),
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::Discover => "aish://discover",
            Self::Status => "aish://status",
            Self::Inspect { .. } => "aish://inspect",
        }
    }

    fn command_name(&self) -> &'static str {
        match self {
            Self::Discover => "aish discover",
            Self::Status => "aish ai status",
            Self::Inspect { .. } => "aish object inspect",
        }
    }

    fn nu_program(&self) -> &'static str {
        match self {
            Self::Discover => "aish discover | to json -r",
            Self::Status => "aish ai status | to json -r",
            Self::Inspect { .. } => {
                "aish object inspect $env.NVIMGL_AISH_KIND $env.NVIMGL_AISH_IDENTITY | to json -r"
            }
        }
    }
}

pub struct Bridge {
    executable: PathBuf,
    cwd: PathBuf,
    tx: Sender<ResultView>,
    rx: Receiver<ResultView>,
}

impl Bridge {
    pub fn new(explicit_executable: Option<PathBuf>) -> Self {
        let executable = explicit_executable
            .or_else(|| std::env::var_os("NVIMGL_AISH_NU").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("aish-nu"));
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (tx, rx) = channel();
        Self {
            executable,
            cwd,
            tx,
            rx,
        }
    }

    pub fn submit(&self, request: Request) {
        let executable = self.executable.clone();
        let cwd = self.cwd.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = execute(&executable, &cwd, request);
            let _ = tx.send(result);
        });
    }

    pub fn submit_error(&self, envelope: ErrorEnvelope) {
        let view = ResultView::from_error("aish://error", "bridge request", envelope);
        let _ = self.tx.send(view);
    }

    pub fn take_results(&self) -> Vec<ResultView> {
        self.rx.try_iter().collect()
    }
}

#[derive(Debug)]
pub struct ResultView {
    pub title: String,
    pub body: String,
}

#[derive(Serialize)]
struct SuccessEnvelope {
    schema: &'static str,
    status: &'static str,
    command: &'static str,
    effect: &'static str,
    execution_authority: &'static str,
    trace_id: String,
    payload: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorEnvelope {
    code: String,
    message: String,
    details: Value,
    trace_id: String,
}

#[derive(Serialize)]
struct FailureEnvelope {
    schema: &'static str,
    status: &'static str,
    command: String,
    effect: &'static str,
    execution_authority: &'static str,
    error: ErrorEnvelope,
}

fn execute(executable: &PathBuf, cwd: &PathBuf, request: Request) -> ResultView {
    let trace_id = Ulid::new().to_string();
    log("info", &trace_id, "aish read-only request started");

    let mut command = Command::new(executable);
    command.arg("-c").arg(request.nu_program()).current_dir(cwd);
    if let Request::Inspect { kind, identity } = &request {
        // Values travel as environment data, never as interpolated Nushell source.
        command.env("NVIMGL_AISH_KIND", kind);
        command.env("NVIMGL_AISH_IDENTITY", identity);
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(io_error) => {
            let envelope = ErrorEnvelope {
                code: "aish_unavailable".to_owned(),
                message: "The configured aish launcher could not be started.".to_owned(),
                details: json!({
                    "io_kind": format!("{:?}", io_error.kind()),
                    "configuration": "Set --aish <path> or NVIMGL_AISH_NU."
                }),
                trace_id: trace_id.clone(),
            };
            log("error", &trace_id, "aish launcher unavailable");
            return ResultView::from_error(request.title(), request.command_name(), envelope);
        }
    };

    if !output.status.success() {
        let envelope = ErrorEnvelope {
            code: "aish_failed".to_owned(),
            message: "The read-only aish request failed.".to_owned(),
            details: json!({
                "exit_status": output.status.code(),
                "stderr": bounded(&String::from_utf8_lossy(&output.stderr))
            }),
            trace_id: trace_id.clone(),
        };
        log("error", &trace_id, "aish read-only request failed");
        return ResultView::from_error(request.title(), request.command_name(), envelope);
    }

    let payload = match serde_json::from_slice::<Value>(&output.stdout) {
        Ok(payload) => payload,
        Err(_) => {
            let envelope = ErrorEnvelope {
                code: "invalid_aish_response".to_owned(),
                message: "Aish did not return the required structured JSON.".to_owned(),
                details: json!({
                    "stdout": bounded(&String::from_utf8_lossy(&output.stdout))
                }),
                trace_id: trace_id.clone(),
            };
            log("error", &trace_id, "aish response was not structured JSON");
            return ResultView::from_error(request.title(), request.command_name(), envelope);
        }
    };

    let envelope = SuccessEnvelope {
        schema: SCHEMA,
        status: "ok",
        command: request.command_name(),
        effect: "read",
        execution_authority: "none",
        trace_id: trace_id.clone(),
        payload,
    };
    log("info", &trace_id, "aish read-only request completed");
    ResultView {
        title: request.title().to_owned(),
        body: serde_json::to_string_pretty(&envelope).expect("serializable aish response"),
    }
}

impl ResultView {
    fn from_error(title: &str, command: &str, error: ErrorEnvelope) -> Self {
        let envelope = FailureEnvelope {
            schema: SCHEMA,
            status: "error",
            command: command.to_owned(),
            effect: "read",
            execution_authority: "none",
            error,
        };
        Self {
            title: title.to_owned(),
            body: serde_json::to_string_pretty(&envelope).expect("serializable bridge error"),
        }
    }
}

fn error(code: &str, message: &str) -> ErrorEnvelope {
    ErrorEnvelope {
        code: code.to_owned(),
        message: message.to_owned(),
        details: json!({}),
        trace_id: Ulid::new().to_string(),
    }
}

fn bounded(value: &str) -> String {
    value.chars().take(MAX_DIAGNOSTIC_CHARS).collect()
}

fn log(level: &str, trace_id: &str, msg: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    eprintln!(
        "{}",
        json!({"ts": ts, "level": level, "trace_id": trace_id, "msg": msg})
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_read_only_surface() {
        assert_eq!(
            Request::from_rpc(&[RpcValue::from("discover")]).unwrap(),
            Request::Discover
        );
        assert_eq!(
            Request::from_rpc(&[
                RpcValue::from("inspect"),
                RpcValue::from("repository"),
                RpcValue::from("."),
            ])
            .unwrap(),
            Request::Inspect {
                kind: "repository".to_owned(),
                identity: ".".to_owned(),
            }
        );
        assert!(Request::from_rpc(&[RpcValue::from("run")]).is_err());
    }

    #[test]
    fn inspect_values_are_not_interpolated_into_nushell_source() {
        let request = Request::Inspect {
            kind: "file".to_owned(),
            identity: "a path with ; rm syntax".to_owned(),
        };
        assert_eq!(
            request.nu_program(),
            "aish object inspect $env.NVIMGL_AISH_KIND $env.NVIMGL_AISH_IDENTITY | to json -r"
        );
        assert!(!request.nu_program().contains("rm syntax"));
    }

    #[test]
    fn bridge_errors_keep_the_required_envelope() {
        let view = ResultView::from_error(
            "aish://error",
            "bridge request",
            error("test_error", "test message"),
        );
        let parsed: Value = serde_json::from_str(&view.body).unwrap();
        assert_eq!(parsed["status"], "error");
        assert_eq!(parsed["effect"], "read");
        assert_eq!(parsed["execution_authority"], "none");
        assert!(parsed["error"]["trace_id"].as_str().is_some());
    }
}
