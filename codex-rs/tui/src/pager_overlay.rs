use std::io::Result;
use std::time::Duration;

use crate::insert_history;
use crate::tui;
use crate::tui::TuiEvent;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::WidgetRef;

pub(crate) enum Overlay {
    Transcript(TranscriptOverlay),
    Static(StaticOverlay),
}

impl Overlay {
    pub(crate) fn new_transcript(lines: Vec<Line<'static>>) -> Self {
        Self::Transcript(TranscriptOverlay::new(lines))
    }

    pub(crate) fn new_static_with_title(lines: Vec<Line<'static>>, title: String) -> Self {
        Self::Static(StaticOverlay::with_title(lines, title))
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match self {
            Overlay::Transcript(o) => o.handle_event(tui, event),
            Overlay::Static(o) => o.handle_event(tui, event),
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        match self {
            Overlay::Transcript(o) => o.is_done(),
            Overlay::Static(o) => o.is_done(),
        }
    }
}

// Common pager navigation hints rendered on the first line
const PAGER_KEY_HINTS: &[(&str, &str)] = &[
    ("↑/↓", "scroll"),
    ("PgUp/PgDn", "page"),
    ("Home/End", "jump"),
];

// Render a single line of key hints from (key, description) pairs.
fn render_key_hints(area: Rect, buf: &mut Buffer, pairs: &[(&str, &str)]) {
    let key_hint_style = Style::default().fg(Color::Cyan);
    let mut spans: Vec<Span<'static>> = vec![" ".into()];
    let mut first = true;
    for (key, desc) in pairs {
        if !first {
            spans.push("   ".into());
        }
        spans.push(Span::from(key.to_string()).set_style(key_hint_style));
        spans.push(" ".into());
        spans.push(Span::from(desc.to_string()));
        first = false;
    }
    Paragraph::new(vec![Line::from(spans).dim()]).render_ref(area, buf);
}

/// Generic widget for rendering a pager view.
struct PagerView {
    lines: Vec<Line<'static>>,
    scroll_offset: usize,
    title: String,
    wrap_cache: Option<WrapCache>,
}

impl PagerView {
    fn new(lines: Vec<Line<'static>>, title: String, scroll_offset: usize) -> Self {
        Self {
            lines,
            scroll_offset,
            title,
            wrap_cache: None,
        }
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.render_header(area, buf);
        let content_area = self.scroll_area(area);
        self.ensure_wrapped(content_area.width);
        // Compute page bounds without holding an immutable borrow on cache while mutating self
        let wrapped_len = self
            .wrap_cache
            .as_ref()
            .map(|c| c.wrapped.len())
            .unwrap_or(0);
        let page_start = self.scroll_offset.min(wrapped_len.saturating_sub(1));
        let page_end = (page_start + content_area.height as usize).min(wrapped_len);
        if let Some(cache) = &self.wrap_cache {
            let visible = &cache.wrapped[page_start..page_end];
            Paragraph::new(visible.to_vec()).render_ref(content_area, buf);
        }
        self.render_scroll_indicator(area, buf, wrapped_len);
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        if area.height >= 2 {
            let header_area = Rect::new(area.x, area.y, area.width, 1);
            let title_style = Style::default().fg(Color::Yellow);
            let title_span = Span::styled(format!(" {} ", self.title), title_style);
            let close_hint = Span::from("  q to close").dim();
            let header_line = Line::from(vec![title_span, close_hint]);
            Paragraph::new(vec![header_line]).render_ref(header_area, buf);
        }
    }

    fn render_scroll_indicator(&self, area: Rect, buf: &mut Buffer, total_lines: usize) {
        if area.height < 3 {
            return;
        }
        let viewport_height = (area.height - 2) as usize;
        if total_lines <= viewport_height {
            return;
        }
        let progress = if self.scroll_offset + viewport_height >= total_lines {
            100
        } else {
            (self.scroll_offset * 100) / total_lines.saturating_sub(viewport_height)
        };
        let indicator_area = Rect::new(area.x + area.width - 10, area.y, 10, 1);
        let indicator = format!(" {:3}% ", progress);
        Paragraph::new(vec![Line::from(indicator).dim()]).render_ref(indicator_area, buf);
    }

    fn scroll_area(&self, area: Rect) -> Rect {
        if area.height >= 2 {
            Rect::new(area.x, area.y + 1, area.width, area.height - 1)
        } else {
            area
        }
    }

    fn ensure_wrapped(&mut self, width: u16) {
        if self.wrap_cache.as_ref().map_or(true, |c| c.width != width) {
            let wrapped = insert_history::word_wrap_lines(&self.lines, width);
            self.wrap_cache = Some(WrapCache { width, wrapped });
        }
    }

    fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
    }

    fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    fn scroll_to_bottom(&mut self, viewport_height: usize) {
        if let Some(cache) = &self.wrap_cache {
            self.scroll_offset = cache.wrapped.len().saturating_sub(viewport_height);
        }
    }
}

struct WrapCache {
    width: u16,
    wrapped: Vec<Line<'static>>,
}

/// Transcript overlay for viewing conversation history
pub(crate) struct TranscriptOverlay {
    pager: PagerView,
    done: bool,
    highlight_range: Option<(usize, usize)>,
}

impl TranscriptOverlay {
    pub(crate) fn new(lines: Vec<Line<'static>>) -> Self {
        Self {
            pager: PagerView::new(lines, "Transcript".to_string(), 0),
            done: false,
            highlight_range: None,
        }
    }

    pub(crate) fn lines(&self) -> &[Line<'static>] {
        &self.pager.lines
    }

    pub(crate) fn set_highlight_range(&mut self, range: Option<(usize, usize)>) {
        self.highlight_range = range;
        if let Some((start, end)) = range {
            // Apply highlight style to lines in range
            for (i, line) in self.pager.lines.iter_mut().enumerate() {
                if i >= start && i < end {
                    *line = line.clone().bg(Color::DarkGray);
                } else {
                    // Remove highlight from lines outside range
                    *line = line.clone().reset();
                }
            }
        }
    }

    pub(crate) fn scroll_to_line(&mut self, line: usize) {
        self.pager.scroll_offset = line;
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => match code {
                KeyCode::Char('q') | KeyCode::Char('Q') => {
                    self.done = true;
                }
                KeyCode::Down => self.pager.scroll_down(1),
                KeyCode::Up => self.pager.scroll_up(1),
                KeyCode::PageDown => {
                    let page_size = (tui.size().height as usize).saturating_sub(3);
                    self.pager.scroll_down(page_size);
                }
                KeyCode::PageUp => {
                    let page_size = (tui.size().height as usize).saturating_sub(3);
                    self.pager.scroll_up(page_size);
                }
                KeyCode::Home => self.pager.scroll_to_top(),
                KeyCode::End => {
                    let viewport_height = (tui.size().height as usize).saturating_sub(2);
                    self.pager.scroll_to_bottom(viewport_height);
                }
                _ => {}
            },
            _ => {}
        }
        tui.request_redraw();
        Ok(())
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.pager.render(area, buf);
    }
}

/// Static overlay for displaying fixed content
pub(crate) struct StaticOverlay {
    pager: PagerView,
    done: bool,
}

impl StaticOverlay {
    pub(crate) fn with_title(lines: Vec<Line<'static>>, title: String) -> Self {
        Self {
            pager: PagerView::new(lines, title, 0),
            done: false,
        }
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => match code {
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                    self.done = true;
                }
                KeyCode::Down => self.pager.scroll_down(1),
                KeyCode::Up => self.pager.scroll_up(1),
                KeyCode::PageDown => {
                    let page_size = (tui.size().height as usize).saturating_sub(3);
                    self.pager.scroll_down(page_size);
                }
                KeyCode::PageUp => {
                    let page_size = (tui.size().height as usize).saturating_sub(3);
                    self.pager.scroll_up(page_size);
                }
                KeyCode::Home => self.pager.scroll_to_top(),
                KeyCode::End => {
                    let viewport_height = (tui.size().height as usize).saturating_sub(2);
                    self.pager.scroll_to_bottom(viewport_height);
                }
                _ => {}
            },
            _ => {}
        }
        tui.request_redraw();
        Ok(())
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.pager.render(area, buf);
    }
}