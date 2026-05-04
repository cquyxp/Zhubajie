use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};

use crossterm::cursor;
use crossterm::execute;
use crossterm::style::Print;
use crossterm::terminal::{self, Clear, ClearType};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{
    Cmd, CompletionType, Config, Context, EditMode, Editor, Helper, KeyCode, KeyEvent, Modifiers,
    Movement, Word,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    Submit(String),
    Cancel,
    Exit,
}

struct SlashCommandHelper {
    completions: Vec<String>,
    current_line: RefCell<String>,
}

impl SlashCommandHelper {
    fn new(completions: Vec<String>) -> Self {
        Self {
            completions: normalize_completions(completions),
            current_line: RefCell::new(String::new()),
        }
    }

    fn reset_current_line(&self) {
        self.current_line.borrow_mut().clear();
    }

    fn current_line(&self) -> String {
        self.current_line.borrow().clone()
    }

    fn set_current_line(&self, line: &str) {
        let mut current = self.current_line.borrow_mut();
        current.clear();
        current.push_str(line);
    }

    fn set_completions(&mut self, completions: Vec<String>) {
        self.completions = normalize_completions(completions);
    }
}

impl Completer for SlashCommandHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let Some(prefix) = slash_command_prefix(line, pos) else {
            return Ok((0, Vec::new()));
        };

        let matches = self
            .completions
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| Pair {
                display: candidate.clone(),
                replacement: candidate.clone(),
            })
            .collect();

        Ok((0, matches))
    }
}

impl Hinter for SlashCommandHelper {
    type Hint = String;
}

impl Highlighter for SlashCommandHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        self.set_current_line(line);
        Cow::Borrowed(line)
    }

    fn highlight_char(&self, line: &str, _pos: usize, _kind: CmdKind) -> bool {
        self.set_current_line(line);
        false
    }
}

impl Validator for SlashCommandHelper {}
impl Helper for SlashCommandHelper {}

/// Sentinel character inserted by Ctrl+O to toggle verbose output mode.
pub const CTRL_O_SENTINEL: char = '\x0f';

pub struct LineEditor {
    prompt: String,
    status_line: Option<String>,
    editor: Editor<SlashCommandHelper, DefaultHistory>,
}

impl LineEditor {
    #[must_use]
    pub fn new(prompt: impl Into<String>, completions: Vec<String>) -> Self {
        let config = Config::builder()
            .completion_type(CompletionType::List)
            .edit_mode(EditMode::Emacs)
            .bracketed_paste(true)
            .build();
        let mut editor = Editor::<SlashCommandHelper, DefaultHistory>::with_config(config)
            .expect("rustyline editor should initialize");
        editor.set_helper(Some(SlashCommandHelper::new(completions)));
        editor.bind_sequence(KeyEvent(KeyCode::Char('J'), Modifiers::CTRL), Cmd::Newline);
        editor.bind_sequence(KeyEvent(KeyCode::Enter, Modifiers::SHIFT), Cmd::Newline);
        // Ctrl+U clears the entire input buffer (upstream: kill-whole-line),
        // and Ctrl+Y restores it (yank). Ctrl+L forces a full redraw.
        editor.bind_sequence(
            KeyEvent(KeyCode::Char('U'), Modifiers::CTRL),
            Cmd::Kill(Movement::WholeLine),
        );
        editor.bind_sequence(
            KeyEvent(KeyCode::Char('L'), Modifiers::CTRL),
            Cmd::ClearScreen,
        );
        // Ctrl+A/E jump to start/end of line (Emacs default, explicit for
        // visibility). Ctrl+W deletes the previous word. Ctrl+Backspace
        // (Windows-compatible) also deletes the previous word.
        editor.bind_sequence(
            KeyEvent(KeyCode::Char('A'), Modifiers::CTRL),
            Cmd::Move(Movement::BeginningOfLine),
        );
        editor.bind_sequence(
            KeyEvent(KeyCode::Char('E'), Modifiers::CTRL),
            Cmd::Move(Movement::EndOfLine),
        );
        editor.bind_sequence(
            KeyEvent(KeyCode::Char('W'), Modifiers::CTRL),
            Cmd::Kill(Movement::BackwardWord(1, Word::Emacs)),
        );
        editor.bind_sequence(
            KeyEvent(KeyCode::Backspace, Modifiers::CTRL),
            Cmd::Kill(Movement::BackwardWord(1, Word::Emacs)),
        );
        // Ctrl+O toggles verbose output mode. Insert a sentinel that the
        // REPL loop detects before processing the line.
        editor.bind_sequence(
            KeyEvent(KeyCode::Char('O'), Modifiers::CTRL),
            Cmd::SelfInsert(1, CTRL_O_SENTINEL),
        );

        Self {
            prompt: prompt.into(),
            status_line: None,
            editor,
        }
    }

    pub fn push_history(&mut self, entry: impl Into<String>) {
        let entry = entry.into();
        if entry.trim().is_empty() {
            return;
        }

        let _ = self.editor.add_history_entry(entry);
    }

    pub fn set_completions(&mut self, completions: Vec<String>) {
        if let Some(helper) = self.editor.helper_mut() {
            helper.set_completions(completions);
        }
    }

    pub fn set_status_line(&mut self, status_line: impl Into<String>) {
        self.status_line = Some(status_line.into());
    }

    pub fn read_line(&mut self) -> io::Result<ReadOutcome> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return self.read_line_fallback();
        }

        if let Some(helper) = self.editor.helper_mut() {
            helper.reset_current_line();
        }

        self.prepare_fixed_status_line()?;

        let result = match self.editor.readline(&self.prompt) {
            Ok(line) => Ok(ReadOutcome::Submit(line)),
            Err(ReadlineError::Interrupted) => {
                let has_input = !self.current_line().is_empty();
                self.finish_interrupted_read()?;
                if has_input {
                    Ok(ReadOutcome::Cancel)
                } else {
                    Ok(ReadOutcome::Exit)
                }
            }
            Err(ReadlineError::Eof) => {
                self.finish_interrupted_read()?;
                Ok(ReadOutcome::Exit)
            }
            Err(error) => Err(io::Error::other(error)),
        };

        self.clear_fixed_status_line()?;
        result
    }

    fn current_line(&self) -> String {
        self.editor
            .helper()
            .map_or_else(String::new, SlashCommandHelper::current_line)
    }

    fn finish_interrupted_read(&mut self) -> io::Result<()> {
        if let Some(helper) = self.editor.helper_mut() {
            helper.reset_current_line();
        }
        let mut stdout = io::stdout();
        writeln!(stdout)
    }

    fn prepare_fixed_status_line(&self) -> io::Result<()> {
        let Some(status_line) = &self.status_line else {
            return Ok(());
        };
        let (width, height) = match terminal::size() {
            Ok(size) if size.1 >= 2 => size,
            _ => return Ok(()),
        };
        let footer_row = height.saturating_sub(1);
        let input_row = height.saturating_sub(2);
        let rendered_status = truncate_status_line(status_line, usize::from(width));
        let mut stdout = io::stdout();

        execute!(
            stdout,
            cursor::MoveTo(0, footer_row),
            Clear(ClearType::CurrentLine),
            Print(rendered_status),
            cursor::MoveTo(0, input_row),
            Clear(ClearType::CurrentLine)
        )?;
        stdout.flush()
    }

    fn clear_fixed_status_line(&self) -> io::Result<()> {
        if self.status_line.is_none() {
            return Ok(());
        }
        let (_, height) = match terminal::size() {
            Ok(size) if size.1 >= 2 => size,
            _ => return Ok(()),
        };
        let footer_row = height.saturating_sub(1);
        let mut stdout = io::stdout();

        execute!(
            stdout,
            cursor::MoveTo(0, footer_row),
            Clear(ClearType::CurrentLine)
        )?;
        stdout.flush()
    }

    fn read_line_fallback(&self) -> io::Result<ReadOutcome> {
        let mut stdout = io::stdout();
        write!(stdout, "{}", self.prompt)?;
        stdout.flush()?;

        let mut buffer = String::new();
        let bytes_read = io::stdin().read_line(&mut buffer)?;
        if bytes_read == 0 {
            return Ok(ReadOutcome::Exit);
        }

        while matches!(buffer.chars().last(), Some('\n' | '\r')) {
            buffer.pop();
        }
        Ok(ReadOutcome::Submit(buffer))
    }
}

fn truncate_status_line(status_line: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let visible = status_line.chars().count();
    if visible <= width {
        return status_line.to_string();
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let mut truncated = status_line
        .chars()
        .take(width.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn slash_command_prefix(line: &str, pos: usize) -> Option<&str> {
    if pos != line.len() {
        return None;
    }

    let prefix = &line[..pos];
    if !prefix.starts_with('/') {
        return None;
    }

    Some(prefix)
}

fn normalize_completions(completions: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    completions
        .into_iter()
        .filter(|candidate| candidate.starts_with('/'))
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{slash_command_prefix, truncate_status_line, LineEditor, SlashCommandHelper};
    use rustyline::completion::Completer;
    use rustyline::highlight::Highlighter;
    use rustyline::history::{DefaultHistory, History};
    use rustyline::Context;

    #[test]
    fn extracts_terminal_slash_command_prefixes_with_arguments() {
        assert_eq!(slash_command_prefix("/he", 3), Some("/he"));
        assert_eq!(slash_command_prefix("/help me", 8), Some("/help me"));
        assert_eq!(
            slash_command_prefix("/session switch ses", 19),
            Some("/session switch ses")
        );
        assert_eq!(slash_command_prefix("hello", 5), None);
        assert_eq!(slash_command_prefix("/help", 2), None);
    }

    #[test]
    fn completes_matching_slash_commands() {
        let helper = SlashCommandHelper::new(vec![
            "/help".to_string(),
            "/hello".to_string(),
            "/status".to_string(),
        ]);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (start, matches) = helper
            .complete("/he", 3, &ctx)
            .expect("completion should work");

        assert_eq!(start, 0);
        assert_eq!(
            matches
                .into_iter()
                .map(|candidate| candidate.replacement)
                .collect::<Vec<_>>(),
            vec!["/help".to_string(), "/hello".to_string()]
        );
    }

    #[test]
    fn completes_matching_slash_command_arguments() {
        let helper = SlashCommandHelper::new(vec![
            "/model".to_string(),
            "/model opus".to_string(),
            "/model sonnet".to_string(),
            "/session switch alpha".to_string(),
        ]);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (start, matches) = helper
            .complete("/model o", 8, &ctx)
            .expect("completion should work");

        assert_eq!(start, 0);
        assert_eq!(
            matches
                .into_iter()
                .map(|candidate| candidate.replacement)
                .collect::<Vec<_>>(),
            vec!["/model opus".to_string()]
        );
    }

    #[test]
    fn ignores_non_slash_command_completion_requests() {
        let helper = SlashCommandHelper::new(vec!["/help".to_string()]);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (_, matches) = helper
            .complete("hello", 5, &ctx)
            .expect("completion should work");

        assert!(matches.is_empty());
    }

    #[test]
    fn tracks_current_buffer_through_highlighter() {
        let helper = SlashCommandHelper::new(Vec::new());
        let _ = helper.highlight("draft", 5);

        assert_eq!(helper.current_line(), "draft");
    }

    #[test]
    fn push_history_ignores_blank_entries() {
        let mut editor = LineEditor::new("> ", vec!["/help".to_string()]);
        editor.push_history("   ");
        editor.push_history("/help");

        assert_eq!(editor.editor.history().len(), 1);
    }

    #[test]
    fn set_completions_replaces_and_normalizes_candidates() {
        let mut editor = LineEditor::new("> ", vec!["/help".to_string()]);
        editor.set_completions(vec![
            "/model opus".to_string(),
            "/model opus".to_string(),
            "status".to_string(),
        ]);

        let helper = editor.editor.helper().expect("helper should exist");
        assert_eq!(helper.completions, vec!["/model opus".to_string()]);
    }

    #[test]
    fn truncates_status_line_to_terminal_width() {
        assert_eq!(truncate_status_line("abcdef", 0), "");
        assert_eq!(truncate_status_line("abcdef", 2), "..");
        assert_eq!(truncate_status_line("abcdef", 4), "a...");
        assert_eq!(truncate_status_line("abc", 4), "abc");
    }
}
