use crate::app::App;
use crate::backtrack_helpers;
use crate::pager_overlay::Overlay;
use crate::tui;
use codex_core::protocol::ConversationHistoryResponseEvent;
use color_eyre::eyre::Result;
use crossterm::event::Event as TuiEvent;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::text::{Line, Span};

/// Aggregates all backtrack-related state used by the App.
#[derive(Default)]
pub(crate) struct BacktrackState {
    /// True when Esc has primed backtrack mode in the main view.
    pub(crate) primed: bool,
    /// Session id of the base conversation to fork from.
    pub(crate) base_id: Option<uuid::Uuid>,
    /// Current step count (Nth last user message).
    pub(crate) count: usize,
    /// True when the transcript overlay is showing a backtrack preview.
    pub(crate) overlay_preview_active: bool,
    /// Pending fork request: (base_id, drop_count, prefill).
    pub(crate) pending: Option<(uuid::Uuid, usize, String)>,
}

impl App<'_> {
    /// Route overlay events when transcript overlay is active.
    /// - If backtrack preview is active: Esc steps selection; Enter confirms.
    /// - Otherwise: Esc begins preview; all other events forward to overlay.
    ///   interactions (Esc to step target, Enter to confirm) and overlay lifecycle.
    pub(crate) async fn handle_backtrack_overlay_event(
        &mut self,
        tui: &mut tui::Tui,
        event: TuiEvent,
    ) -> Result<bool> {
        if self.backtrack.overlay_preview_active {
            match event {
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press | KeyEventKind::Repeat,
                    ..
                }) => {
                    self.overlay_step_backtrack(tui, event)?;
                    Ok(true)
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    self.overlay_confirm_backtrack(tui);
                    Ok(true)
                }
                // Catchall: forward any other events to the overlay widget.
                _ => {
                    self.overlay_forward_event(tui, event)?;
                    Ok(true)
                }
            }
        } else if let TuiEvent::Key(KeyEvent {
            code: KeyCode::Esc,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        }) = event
        {
            // First Esc in transcript overlay: begin backtrack preview at latest user message.
            self.begin_overlay_backtrack_preview(tui);
            Ok(true)
        } else {
            // Not in backtrack mode: forward events to the overlay widget.
            self.overlay_forward_event(tui, event)?;
            Ok(true)
        }
    }

    /// Handle global Esc presses for backtracking when no overlay is present.
    pub(crate) fn handle_backtrack_esc_key(&mut self, tui: &mut tui::Tui) {
        // Only handle backtracking when composer is empty to avoid clobbering edits.
        let composer_is_empty = self.chat_widget.as_ref().map_or(false, |w| w.composer_is_empty());
        if composer_is_empty {
            if !self.backtrack.primed {
                self.prime_backtrack();
            } else if self.overlay.is_none() {
                self.open_backtrack_preview(tui);
            } else if self.backtrack.overlay_preview_active {
                self.step_backtrack_and_highlight(tui);
            }
        }
    }

    /// Stage a backtrack and request conversation history from the agent.
    pub(crate) fn request_backtrack(
        &mut self,
        prefill: String,
        base_id: uuid::Uuid,
        drop_last_messages: usize,
    ) {
        self.backtrack.pending = Some((base_id, drop_last_messages, prefill));
        self.app_event_tx.send(crate::app_event::AppEvent::CodexOp(
            codex_core::protocol::Op::GetHistory,
        ));
    }

    /// Apply a pending backtrack fork if the history response matches.
    pub(crate) fn apply_pending_backtrack(
        &mut self,
        history: ConversationHistoryResponseEvent,
    ) {
        if let Some((base_id, drop_last_messages, prefill)) = self.backtrack.pending.take() {
            if let Some(current_session) = &history
                .conversations
                .iter()
                .find(|c| c.active)
                .map(|c| c.session_id)
            {
                if current_session == &base_id {
                    self.app_event_tx.send(crate::app_event::AppEvent::CodexOp(
                        codex_core::protocol::Op::ForkConversation {
                            base_session_id: base_id,
                            drop_last_messages,
                            initial_message: prefill,
                        },
                    ));
                }
            }
        }
    }

    /// Prime backtrack mode (first Esc).
    fn prime_backtrack(&mut self) {
        self.backtrack.primed = true;
        self.backtrack.count = 1;
        // TODO: Implement proper session ID retrieval when available
        // For now, use a placeholder or skip session management
        // self.backtrack.base_id = None;
    }

    /// Open transcript overlay with backtrack preview (second Esc).
    fn open_backtrack_preview(&mut self, tui: &mut tui::Tui) {
        if self.overlay.is_none() {
            // Use transcript_lines instead of getting from chat_widget
            let lines = self.transcript_lines.clone();
            self.overlay = Some(Overlay::new_transcript(lines));
            self.backtrack.overlay_preview_active = true;
            self.step_backtrack_and_highlight(tui);
        }
    }

    /// Begin backtrack preview when already in transcript overlay.
    fn begin_overlay_backtrack_preview(&mut self, tui: &mut tui::Tui) {
        self.backtrack.primed = true;
        self.backtrack.count = 1;
        self.backtrack.overlay_preview_active = true;
        // TODO: Implement proper session ID retrieval when available
        // self.backtrack.base_id = None;
        self.step_backtrack_and_highlight(tui);
    }

    /// Step through backtrack selections and update overlay.
    fn overlay_step_backtrack(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        self.backtrack.count = self.backtrack.count.saturating_add(1);
        self.step_backtrack_and_highlight(tui);
        self.overlay_forward_event(tui, event)
    }

    /// Confirm the backtrack selection and initiate fork.
    fn overlay_confirm_backtrack(&mut self, tui: &mut tui::Tui) {
        let backtrack_info = if let Some(Overlay::Transcript(ref transcript)) = self.overlay {
            let lines = transcript.lines();
            backtrack_helpers::nth_last_user_text(lines, self.backtrack.count)
                .map(|prefill| (prefill, self.backtrack.base_id, self.backtrack.count))
        } else {
            None
        };
        
        if let Some((prefill, Some(base_id), count)) = backtrack_info {
            self.request_backtrack(prefill, base_id, count);
        }
        
        self.close_overlay(tui);
        self.reset_backtrack();
    }

    /// Update overlay highlight based on current backtrack step.
    fn step_backtrack_and_highlight(&mut self, tui: &mut tui::Tui) {
        if let Some(Overlay::Transcript(ref mut transcript)) = self.overlay {
            // Clone the lines to avoid multiple borrows
            let lines_clone: Vec<Line<'static>> = transcript.lines().iter().cloned().map(|line| {
                let owned_spans: Vec<Span<'static>> = line.spans.iter()
                    .map(|s| {
                        let mut span = Span::from(s.content.to_string());
                        span.style = s.style;
                        span
                    })
                    .collect();
                Line::from(owned_spans)
            }).collect();
            
            let n = backtrack_helpers::normalize_backtrack_n(&lines_clone, self.backtrack.count);
            self.backtrack.count = n;
            if let Some((start, end)) = backtrack_helpers::highlight_range_for_nth_last_user(&lines_clone, n) {
                transcript.set_highlight_range(Some((start, end)));
                let wrapped_offset = backtrack_helpers::wrapped_offset_before(
                    &lines_clone,
                    start,
                    tui.size().unwrap_or_default().width,
                );
                transcript.scroll_to_line(wrapped_offset);
            }
        }
    }

    /// Forward events to the overlay widget.
    fn overlay_forward_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        if let Some(ref mut overlay) = self.overlay {
            overlay.handle_event(tui, event)?;
            if overlay.is_done() {
                self.close_overlay(tui);
            }
        }
        Ok(())
    }

    /// Close overlay and reset state.
    fn close_overlay(&mut self, _tui: &mut tui::Tui) {
        self.overlay = None;
        // Note: redraw will be handled by the main event loop
    }

    /// Reset all backtrack state.
    fn reset_backtrack(&mut self) {
        self.backtrack = BacktrackState::default();
    }
}