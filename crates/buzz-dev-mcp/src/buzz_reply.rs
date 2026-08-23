use crate::shell::SharedState;
use rmcp::model::CallToolResult;
use rmcp::ErrorData;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const READ_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuzzReplyParams {
    /// Reply text, including any Markdown the user should see.
    pub content: String,
    /// Buzz channel UUID that should receive the reply.
    pub channel: String,
    /// Source Buzz event ID. Include this when responding to a message so the
    /// published event is attached to the correct thread.
    #[serde(default)]
    pub reply_to: Option<String>,
}

pub async fn run(
    state: &SharedState,
    p: BuzzReplyParams,
    ct: CancellationToken,
) -> Result<CallToolResult, ErrorData> {
    let mut cmd = Command::new(state.shim.buzz_path());
    cmd.args(command_args(&p));
    cmd.current_dir(&state.cwd);
    cmd.env("PATH", &state.shim.path_env);
    // Do not clear the environment: BUZZ_PRIVATE_KEY, BUZZ_RELAY_URL, and
    // BUZZ_AUTH_TAG are intentionally inherited from the ACP harness.
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    crate::configure_no_window_async(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Ok(tool_error(
                "buzz_cli_spawn_error",
                format!("failed to start bundled buzz CLI: {error}"),
            ));
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = tokio::spawn(async move { read_bounded(stdout).await });
    let stderr_task = tokio::spawn(async move { read_bounded(stderr).await });

    let status = tokio::select! {
        biased;
        _ = ct.cancelled() => {
            reap(&mut child).await;
            stdout_task.abort();
            stderr_task.abort();
            return Ok(tool_error("cancelled", "Buzz reply was cancelled"));
        }
        result = tokio::time::timeout(TIMEOUT, child.wait()) => {
            match result {
                Ok(Ok(status)) => status,
                Ok(Err(error)) => {
                    reap(&mut child).await;
                    stdout_task.abort();
                    stderr_task.abort();
                    return Ok(tool_error(
                        "buzz_cli_wait_error",
                        format!("failed while waiting for buzz CLI: {error}"),
                    ));
                }
                Err(_) => {
                    reap(&mut child).await;
                    stdout_task.abort();
                    stderr_task.abort();
                    return Ok(tool_error(
                        "buzz_cli_timeout",
                        "buzz messages send timed out after 120 seconds",
                    ));
                }
            }
        }
    };

    let stdout = join_capture(stdout_task, "stdout").await;
    let stderr = join_capture(stderr_task, "stderr").await;
    Ok(result_from_output(
        status.code().unwrap_or(-1),
        stdout,
        stderr,
    ))
}

fn command_args(p: &BuzzReplyParams) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("messages"),
        OsString::from("send"),
        OsString::from("--channel"),
        OsString::from(&p.channel),
        OsString::from("--content"),
        OsString::from(&p.content),
    ];
    if let Some(reply_to) = &p.reply_to {
        args.push(OsString::from("--reply-to"));
        args.push(OsString::from(reply_to));
    }
    args
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
    read_error: Option<String>,
}

async fn read_bounded<R>(stream: Option<R>) -> Captured
where
    R: AsyncRead + Unpin,
{
    let Some(mut stream) = stream else {
        return Captured::default();
    };
    let mut capture = Captured::default();
    let mut chunk = vec![0; READ_CHUNK_BYTES];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(read) => {
                let available = MAX_OUTPUT_BYTES.saturating_sub(capture.bytes.len());
                let keep = read.min(available);
                capture.bytes.extend_from_slice(&chunk[..keep]);
                capture.truncated |= keep < read;
            }
            Err(error) => {
                capture.read_error = Some(error.to_string());
                break;
            }
        }
    }
    capture
}

async fn join_capture(task: tokio::task::JoinHandle<Captured>, label: &str) -> Captured {
    match task.await {
        Ok(capture) => capture,
        Err(error) => Captured {
            read_error: Some(format!("failed to capture buzz CLI {label}: {error}")),
            ..Captured::default()
        },
    }
}

async fn reap(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
}

fn result_from_output(exit_code: i32, stdout: Captured, stderr: Captured) -> CallToolResult {
    if stdout.truncated || stderr.truncated {
        return tool_error(
            "buzz_cli_output_too_large",
            format!("buzz CLI output exceeded {MAX_OUTPUT_BYTES} bytes"),
        );
    }
    if let Some(error) = stdout.read_error.or(stderr.read_error) {
        return tool_error("buzz_cli_output_error", error);
    }

    let stdout = String::from_utf8_lossy(&stdout.bytes);
    let stderr = String::from_utf8_lossy(&stderr.bytes);
    if exit_code == 0 {
        return match serde_json::from_str::<Value>(stdout.trim()) {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => tool_error(
                "buzz_cli_invalid_output",
                format!("buzz CLI returned invalid JSON: {error}"),
            ),
        };
    }

    let error_text = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    match serde_json::from_str::<Value>(error_text) {
        Ok(value) => CallToolResult::structured_error(value),
        Err(_) => CallToolResult::structured_error(json!({
            "error": "buzz_cli_error",
            "message": error_text,
            "exit_code": exit_code,
        })),
    }
}

fn tool_error(error: &str, message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({
        "error": error,
        "message": message.into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(reply_to: Option<&str>) -> BuzzReplyParams {
        BuzzReplyParams {
            content: "Reply with `code` and $variables; unchanged".into(),
            channel: "123e4567-e89b-12d3-a456-426614174000".into(),
            reply_to: reply_to.map(str::to_owned),
        }
    }

    fn capture(value: &str) -> Captured {
        Captured {
            bytes: value.as_bytes().to_vec(),
            ..Captured::default()
        }
    }

    #[test]
    fn builds_exact_cli_arguments_without_shell_interpolation() {
        let p = params(Some(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ));
        assert_eq!(
            command_args(&p),
            vec![
                "messages",
                "send",
                "--channel",
                "123e4567-e89b-12d3-a456-426614174000",
                "--content",
                "Reply with `code` and $variables; unchanged",
                "--reply-to",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn omits_reply_to_when_not_provided() {
        let args = command_args(&params(None));
        assert!(!args.iter().any(|arg| arg == "--reply-to"));
    }

    #[test]
    fn returns_successful_cli_json_as_structured_content() {
        let value = json!({
            "event_id": "abc123",
            "accepted": true,
            "message": "",
            "mention_pubkeys": [],
        });
        let result = result_from_output(0, capture(&value.to_string()), Captured::default());
        assert_eq!(result.structured_content, Some(value));
        assert_eq!(result.is_error, Some(false));
    }

    #[test]
    fn preserves_structured_cli_errors() {
        let value = json!({
            "error": "auth_error",
            "message": "BUZZ_PRIVATE_KEY is required",
            "retryable": false,
        });
        let result = result_from_output(3, Captured::default(), capture(&value.to_string()));
        assert_eq!(result.structured_content, Some(value));
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn rejects_oversized_cli_output() {
        let result = result_from_output(
            0,
            Captured {
                truncated: true,
                ..Captured::default()
            },
            Captured::default(),
        );
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value["error"].as_str()),
            Some("buzz_cli_output_too_large")
        );
    }
}
