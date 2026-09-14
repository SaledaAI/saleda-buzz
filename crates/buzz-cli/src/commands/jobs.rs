use buzz_taskboard::{JobLifecycleRefs, JobPhase, JobUpdateV1};
use nostr::{EventBuilder, Kind, Tag};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{read_or_stdin, validate_hex64};
use crate::{JobsCmd, TaskJobPhaseArg};

fn phase(value: TaskJobPhaseArg) -> (JobPhase, u32) {
    match value {
        TaskJobPhaseArg::Accepted => (JobPhase::Accepted, 43002),
        TaskJobPhaseArg::WaitingForCapacity => (JobPhase::WaitingForCapacity, 43003),
        TaskJobPhaseArg::WaitingForModel => (JobPhase::WaitingForModel, 43003),
        TaskJobPhaseArg::Running => (JobPhase::Running, 43003),
        TaskJobPhaseArg::ReviewReady => (JobPhase::ReviewReady, 43003),
        TaskJobPhaseArg::Cancelled => (JobPhase::Cancelled, 43003),
        TaskJobPhaseArg::Failed => (JobPhase::Failed, 43006),
        TaskJobPhaseArg::Completed => (JobPhase::Completed, 43004),
    }
}

fn tags(request: &str, issue: &str, channel: Option<&str>) -> Result<Vec<Tag>, CliError> {
    validate_hex64(request)?;
    validate_hex64(issue)?;
    let mut raw = JobLifecycleRefs {
        request_id: request.to_string(),
        issue_id: issue.to_string(),
    }
    .to_tags()
    .map_err(|error| CliError::Usage(error.to_string()))?;
    if let Some(channel) = channel {
        uuid::Uuid::parse_str(channel)
            .map_err(|_| CliError::Usage("--channel must be a UUID".into()))?;
        raw.push(vec!["h".into(), channel.into()]);
    }
    raw.into_iter()
        .map(|tag| {
            Tag::parse(tag).map_err(|error| CliError::Other(format!("invalid job tag: {error}")))
        })
        .collect()
}

async fn update(
    client: &BuzzClient,
    request: &str,
    issue: &str,
    phase_arg: TaskJobPhaseArg,
    summary: &str,
    channel: Option<&str>,
) -> Result<(), CliError> {
    let summary = read_or_stdin(summary)?;
    let (phase, kind) = phase(phase_arg);
    let payload = JobUpdateV1::new(phase, summary)
        .and_then(|update| update.to_json())
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let event = EventBuilder::new(Kind::Custom(kind as u16), payload)
        .tags(tags(request, issue, channel)?)
        .sign_with_keys(client.keys())
        .map_err(|error| CliError::Other(format!("job event signing failed: {error}")))?;
    let response = client.submit_event(event).await?;
    println!("{response}");
    Ok(())
}

async fn cancel(
    client: &BuzzClient,
    request: &str,
    issue: &str,
    executor: &str,
    channel: Option<&str>,
) -> Result<(), CliError> {
    validate_hex64(executor)?;
    let mut event_tags = tags(request, issue, channel)?;
    event_tags.push(
        Tag::parse(["p", executor])
            .map_err(|error| CliError::Other(format!("invalid executor tag: {error}")))?,
    );
    let content = serde_json::json!({
        "v": 1,
        "operation": "cancel",
        "summary": "Cancellation requested by the task owner."
    })
    .to_string();
    let event = EventBuilder::new(Kind::Custom(43005), content)
        .tags(event_tags)
        .sign_with_keys(client.keys())
        .map_err(|error| CliError::Other(format!("job cancellation signing failed: {error}")))?;
    let response = client.submit_event(event).await?;
    println!("{response}");
    Ok(())
}

pub async fn dispatch(command: JobsCmd, client: &BuzzClient) -> Result<(), CliError> {
    match command {
        JobsCmd::Update {
            request,
            issue,
            phase,
            summary,
            channel,
        } => {
            update(
                client,
                &request,
                &issue,
                phase,
                &summary,
                channel.as_deref(),
            )
            .await
        }
        JobsCmd::Cancel {
            request,
            issue,
            executor,
            channel,
        } => cancel(client, &request, &issue, &executor, channel.as_deref()).await,
    }
}
