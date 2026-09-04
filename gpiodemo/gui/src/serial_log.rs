use std::{collections::VecDeque, sync::Arc};

use chrono::Local;
use iced::widget::text_editor::{self, Action, Edit, Motion};

const MAX_LINES: usize = 2_000;
const RETAIN_LINES_AFTER_TRIM: usize = 1_500;

#[derive(Clone)]
struct Entry {
    timestamp: String,
    text: String,
}

pub(super) struct SerialLog {
    entries: VecDeque<Entry>,
    content: text_editor::Content,
    show_timestamps: bool,
    pending_text: String,
    rebuild: bool,
}

impl SerialLog {
    pub(super) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            content: text_editor::Content::new(),
            show_timestamps: true,
            pending_text: String::new(),
            rebuild: false,
        }
    }

    pub(super) fn push(&mut self, text: String) {
        let entry = Entry {
            timestamp: Local::now().format("%H:%M:%S%.3f").to_string(),
            text,
        };

        if !self.rebuild {
            format_entry(&entry, self.show_timestamps, &mut self.pending_text);
        }
        self.entries.push_back(entry);

        if self.entries.len() > MAX_LINES {
            while self.entries.len() > RETAIN_LINES_AFTER_TRIM {
                self.entries.pop_front();
            }
            self.pending_text.clear();
            self.rebuild = true;
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.pending_text.clear();
        self.rebuild = true;
    }

    pub(super) fn set_show_timestamps(&mut self, enabled: bool) {
        if self.show_timestamps != enabled {
            self.show_timestamps = enabled;
            self.pending_text.clear();
            self.rebuild = true;
        }
    }

    pub(super) fn show_timestamps(&self) -> bool {
        self.show_timestamps
    }

    pub(super) fn content(&self) -> &text_editor::Content {
        &self.content
    }

    pub(super) fn perform(&mut self, action: Action) {
        self.content.perform(action);
    }

    pub(super) fn flush(&mut self) -> bool {
        if self.rebuild {
            self.rebuild_content();
            self.rebuild = false;
            return true;
        }

        if self.pending_text.is_empty() {
            return false;
        }

        let cursor = self.content.cursor();
        self.content.perform(Action::Move(Motion::DocumentEnd));
        self.content
            .perform(Action::Edit(Edit::Paste(Arc::new(std::mem::take(
                &mut self.pending_text,
            )))));
        self.content.move_to(cursor);
        true
    }

    fn rebuild_content(&mut self) {
        let mut text = String::new();
        for entry in &self.entries {
            format_entry(entry, self.show_timestamps, &mut text);
        }
        self.content = text_editor::Content::with_text(&text);
        self.pending_text.clear();
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub(super) fn last_text(&self) -> Option<&str> {
        self.entries.back().map(|entry| entry.text.as_str())
    }

    #[cfg(test)]
    pub(super) fn iter(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.text.as_str())
    }
}

fn format_entry(entry: &Entry, show_timestamps: bool, output: &mut String) {
    if show_timestamps {
        output.push('[');
        output.push_str(&entry.timestamp);
        output.push_str("] ");
    }
    output.push_str(&entry.text);
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_appends_until_flush() {
        let mut log = SerialLog::new();
        for index in 0..200 {
            log.push(format!("RX {index}"));
        }

        assert_eq!(log.content().text(), "");
        assert!(log.flush());
        assert_eq!(log.content().line_count(), 201);
        assert!(log.content().text().contains("RX 199"));
    }

    #[test]
    fn trims_in_chunks_instead_of_rebuilding_every_line() {
        let mut log = SerialLog::new();
        for index in 0..=MAX_LINES {
            log.push(format!("RX {index}"));
        }

        assert_eq!(log.entries.len(), RETAIN_LINES_AFTER_TRIM);
        assert!(log.rebuild);
        assert!(log.flush());
        assert!(log.content().text().contains("RX 2000"));
        assert!(!log.content().text().contains("RX 0\n"));
    }
}
