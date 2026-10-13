//! Lazy, pull-based event streams sharing the client's HTTP and wait budgets.
use std::collections::VecDeque;
use std::future::Future;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_core::Stream;
use futures_util::{stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::wait::PollBudget;
use crate::{Api, Error, EventsQuery, PromptRequest, Result, SandboxEvent, WaitOptions};

#[derive(Clone, Debug)]
pub struct StreamEventsOptions {
    pub wait: WaitOptions,
    pub type_: Option<String>,
    pub limit: u32,
    pub include_existing: bool,
    pub cancellation: Option<CancellationToken>,
}
impl Default for StreamEventsOptions {
    fn default() -> Self {
        Self {
            wait: WaitOptions::from_millis(0, 1_000),
            type_: None,
            limit: 100,
            include_existing: false,
            cancellation: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StreamPromptOptions {
    pub wait: WaitOptions,
    pub cancellation: Option<CancellationToken>,
}
impl Default for StreamPromptOptions {
    fn default() -> Self {
        Self {
            wait: WaitOptions::from_millis(1_800_000, 1_000),
            cancellation: None,
        }
    }
}

/// Poll new event pages in ascending order. No work starts until the stream is
/// polled. Dropping it cancels local work; the token can interrupt in-flight I/O.
pub fn stream_events(
    api: &Api,
    sandbox_id: &str,
    options: Option<StreamEventsOptions>,
) -> impl Stream<Item = Result<SandboxEvent>> + Send + 'static {
    make_stream(EventStreamState::new(
        api.clone(),
        sandbox_id.into(),
        options.unwrap_or_default(),
        None,
    ))
}

/// Queue exactly one prompt and yield only its events, draining all pages before
/// completion. Cancellation does not undo an already accepted prompt.
pub fn stream_prompt(
    api: &Api,
    sandbox_id: &str,
    request: PromptRequest,
    options: Option<StreamPromptOptions>,
) -> impl Stream<Item = Result<SandboxEvent>> + Send + 'static {
    let opts = options.unwrap_or_default();
    make_stream(EventStreamState::new(
        api.clone(),
        sandbox_id.into(),
        StreamEventsOptions {
            wait: opts.wait,
            cancellation: opts.cancellation,
            type_: Some("prompt,response,usage_limit,shield".into()),
            ..Default::default()
        },
        Some(request),
    ))
}

fn make_stream(
    state: EventStreamState,
) -> impl Stream<Item = Result<SandboxEvent>> + Send + 'static {
    stream::try_unfold(state, |mut state| async move {
        let event = state.next_event().await?;
        Ok(event.map(|event| (event, state)))
    })
    .fuse()
}

struct EventStreamState {
    api: Api,
    sandbox_id: String,
    options: StreamEventsOptions,
    request: Option<PromptRequest>,
    prompt_id: Option<String>,
    budget: Option<PollBudget>,
    cursor: Option<String>,
    pending: VecDeque<SandboxEvent>,
    after_page: bool,
    more: bool,
    terminal_event: bool,
    terminal_run: bool,
}
impl EventStreamState {
    fn new(
        api: Api,
        sandbox_id: String,
        options: StreamEventsOptions,
        request: Option<PromptRequest>,
    ) -> Self {
        Self {
            api,
            sandbox_id,
            options,
            request,
            prompt_id: None,
            budget: None,
            cursor: None,
            pending: VecDeque::new(),
            after_page: false,
            more: false,
            terminal_event: false,
            terminal_run: false,
        }
    }

    async fn run<T>(&self, future: impl Future<Output = Result<T>>) -> Result<T> {
        let cancelled = async {
            match &self.options.cancellation {
                Some(token) => token.cancelled().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            _=cancelled=>Err(Error::Aborted),
            result=self.budget.as_ref().expect("stream initialized").run(&self.sandbox_id,"event stream",future)=>result,
        }
    }

    async fn initialize(&mut self) -> Result<()> {
        if self.options.limit == 0 {
            return Err(Error::Config("event stream limit must be positive".into()));
        }
        self.budget = Some(PollBudget::new(self.options.wait.timeout)?);
        // Start after the latest event so existing history is excluded at any size.
        if !self.options.include_existing || self.request.is_some() {
            let query = EventsQuery {
                conversation: None,
                limit: Some(1),
                sort: Some("desc".into()),
                type_: if self.request.is_some() {
                    None
                } else {
                    self.options.type_.clone()
                },
                ..Default::default()
            };
            let existing = self
                .run(self.api.events(&self.sandbox_id, Some(&query)))
                .await?;
            if let Some(last) = existing.events.first() {
                self.cursor = event_cursor(last)
                    .or_else(|| existing.page_info.and_then(|page| page.next_cursor));
                if self.cursor.is_none() {
                    return Err(Error::Unexpected(
                        "existing event has no usable cursor".into(),
                    ));
                }
            }
        }
        if let Some(request) = self.request.take() {
            let response = self.run(self.api.prompt(&self.sandbox_id, request)).await?;
            self.prompt_id = Some(response.prompt_id);
        }
        Ok(())
    }

    async fn next_event(&mut self) -> Result<Option<SandboxEvent>> {
        if self.budget.is_none() {
            self.initialize().await?;
        }
        loop {
            // Check cancellation/deadline even while consuming a buffered page.
            self.run(async { Ok(()) }).await?;
            if let Some(event) = self.pending.pop_front() {
                if self.prompt_id.is_some()
                    && event.type_ == "prompt"
                    && event
                        .data
                        .as_ref()
                        .and_then(|data| data.get("status"))
                        .and_then(|v| v.as_str())
                        .is_some_and(is_terminal)
                {
                    self.terminal_event = true;
                }
                return Ok(Some(event));
            }
            if self.after_page && !self.more {
                if let Some(prompt_id) = &self.prompt_id {
                    if self.terminal_event || self.terminal_run {
                        return Ok(None);
                    }
                    let run = self
                        .run(self.api.prompt_run_status(&self.sandbox_id, prompt_id))
                        .await?
                        .prompt_run;
                    if is_terminal(&run.status) {
                        // Status can race with persistence of the final event page.
                        // Refresh once, then drain all returned pages before ending.
                        self.terminal_run = true;
                    } else {
                        self.pause().await?;
                    }
                } else {
                    self.pause().await?;
                }
            }
            let query = EventsQuery {
                conversation: None,
                limit: Some(self.options.limit),
                cursor: self.cursor.clone(),
                sort: Some("asc".into()),
                type_: self.options.type_.clone(),
            };
            let page = self
                .run(self.api.events(&self.sandbox_id, Some(&query)))
                .await?;
            self.more = page
                .page_info
                .as_ref()
                .map(|page| page.has_more)
                .unwrap_or(page.events.len() >= self.options.limit as usize);
            let next = page
                .page_info
                .and_then(|page| page.next_cursor)
                .filter(|cursor| !cursor.is_empty())
                .or_else(|| page.events.last().and_then(event_cursor));
            if !page.events.is_empty() || self.more {
                if next.is_none() || next == self.cursor {
                    return Err(Error::Unexpected(
                        "event page did not advance its cursor".into(),
                    ));
                } else {
                    self.cursor = next;
                }
            }
            self.pending = page
                .events
                .into_iter()
                .filter(|event| {
                    self.prompt_id
                        .as_ref()
                        .is_none_or(|id| event.task_id.as_ref() == Some(id))
                })
                .collect();
            self.after_page = true;
        }
    }

    async fn pause(&self) -> Result<()> {
        self.run(async {
            tokio::time::sleep(self.options.wait.poll_interval).await;
            Ok(())
        })
        .await
    }
}
fn is_terminal(status: &str) -> bool {
    matches!(status, "finished" | "failed")
}
fn event_cursor(event: &SandboxEvent) -> Option<String> {
    let timestamp = event.timestamp.filter(|value| value.is_finite())?;
    let id = event.id.as_ref().filter(|id| !id.is_empty())?;
    Some(URL_SAFE_NO_PAD.encode(format!("{timestamp}:{id}")))
}
