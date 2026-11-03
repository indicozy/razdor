//! A program in a pseudo-terminal, its screen kept by a VT100 model (`vt100`, with 2000 lines of
//! scrollback): the Claude dock's session. A reader thread takes the program's output off the
//! pty; the window feeds it to the model when it draws (`pump`), and keys go in with `write`. No
//! window here. Desktop only (the pty is `portable-pty`: Linux, macOS, Windows ConPTY).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// The scrollback kept above the screen, in lines.
pub const SCROLLBACK: usize = 2000;

/// What to run: the program, its arguments, its folder, and its extra environment (on top of the
/// inherited one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

/// A running program in a pty and its screen.
pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// The output as the reader thread takes it; `None` when the program's output has ended.
    output: Receiver<Option<Vec<u8>>>,
    parser: vt100::Parser,
    rows: u16,
    cols: u16,
    /// Set once the output ended (the program exited or closed its terminal).
    ended: bool,
    /// The exit status, as text, once the program has ended.
    exit: Option<String>,
}

impl Terminal {
    /// Runs `command` in a pty of `rows` × `cols` cells.
    pub fn spawn(command: &Command, rows: u16, cols: u16) -> Result<Terminal, String> {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("cannot open a terminal: {e}"))?;
        let mut cmd = CommandBuilder::new(&command.program);
        cmd.args(&command.args);
        cmd.cwd(&command.cwd);
        // A pty needs a terminal type the screen model speaks; the rest of the environment is inherited.
        cmd.env("TERM", "xterm-256color");
        for (k, v) in &command.env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("cannot start {}: {e}", command.program.display()))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| format!("cannot read the terminal: {e}"))?;
        let writer = pair.master.take_writer().map_err(|e| format!("cannot write to the terminal: {e}"))?;
        let (tx, output) = channel();
        thread::Builder::new()
            .name("studio-terminal".into())
            .spawn(move || {
                let mut buf = vec![0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => {
                            let _ = tx.send(None);
                            break;
                        }
                        Ok(n) => {
                            if tx.send(Some(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| format!("cannot start the terminal's reader: {e}"))?;
        let parser = vt100::Parser::new(nz(rows), nz(cols), SCROLLBACK);
        Ok(Terminal { master: pair.master, writer, child, output, parser, rows, cols, ended: false, exit: None })
    }

    /// Takes the output the program has written and feeds it to the screen. True when it changed.
    /// Also notes the program's exit once it has one.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.output.try_recv() {
                Ok(Some(bytes)) => {
                    self.parser.process(&bytes);
                    changed = true;
                }
                Ok(None) | Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if self.exit.is_none() {
            self.poll_exit();
        }
        changed
    }

    fn poll_exit(&mut self) {
        if let Ok(Some(status)) = self.child.try_wait() {
            let text = if status.success() { "exit 0".to_string() } else { format!("exit {}", status.exit_code()) };
            self.exit = Some(text);
        }
    }

    /// Whether the program is still running (its exit noted when it has ended).
    pub fn running(&mut self) -> bool {
        if self.exit.is_none() {
            self.poll_exit();
        }
        self.exit.is_none()
    }

    /// The exit status, once the program has ended.
    pub fn exit_status(&self) -> Option<&str> {
        self.exit.as_deref()
    }

    /// The screen: its cells, cursor and modes (the window draws it).
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// The screen's size in cells: rows, columns.
    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }

    /// Sends bytes to the program (keys, pasted text). Typing goes back to the screen's bottom.
    pub fn write(&mut self, bytes: &[u8]) {
        if self.exit.is_some() || bytes.is_empty() {
            return;
        }
        self.parser.screen_mut().set_scrollback(0);
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Resizes the pty and the screen to `rows` × `cols` cells (nothing when they are the same).
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        if (rows, cols) == (self.rows, self.cols) {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.parser.set_size(nz(rows), nz(cols));
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }

    /// Moves the view `lines` lines up (positive) or down (negative) in the scrollback.
    pub fn scroll(&mut self, lines: isize) {
        let screen = self.parser.screen_mut();
        let now = screen.scrollback() as isize;
        screen.set_scrollback((now + lines).max(0) as usize);
    }

    /// The view's distance from the bottom, in lines (0 is the live screen).
    pub fn scrolled(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// Kills the program (and nothing more). Safe to call twice.
    pub fn kill(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.poll_exit();
            if self.exit.is_none() {
                self.exit = Some("killed".into());
            }
        }
    }

    /// Whether the output has ended (the window shows the exit status then).
    pub fn ended(&self) -> bool {
        self.ended
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.kill();
    }
}

fn nz(n: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(n.max(1)).expect("at least one cell")
}

/// A key the terminal takes, as the window names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    /// Shift+Tab.
    BackTab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    /// A letter with Ctrl held (`'c'` for Ctrl+C); Ctrl+C is sent as the interrupt of the window's copy.
    Ctrl(char),
}

/// The bytes a key sends. `app_cursor` is the program's application cursor mode (arrows as `ESC O x`).
pub fn key_bytes(key: Key, app_cursor: bool) -> Vec<u8> {
    let arrow = |c: u8| if app_cursor { vec![0x1b, b'O', c] } else { vec![0x1b, b'[', c] };
    // `ESC [ n ~` with n in decimal, unpadded.
    let tilde = |n: u8| format!("\x1b[{n}~").into_bytes();
    match key {
        Key::Enter => b"\r".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::BackTab => b"\x1b[Z".to_vec(),
        Key::Backspace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::Up => arrow(b'A'),
        Key::Down => arrow(b'B'),
        Key::Right => arrow(b'C'),
        Key::Left => arrow(b'D'),
        Key::Home => arrow(b'H'),
        Key::End => arrow(b'F'),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::Delete => tilde(3),
        Key::Insert => tilde(2),
        Key::Ctrl(c) => {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() {
                vec![c as u8 - b'a' + 1]
            } else {
                match c {
                    '[' => vec![0x1b],
                    '\\' => vec![0x1c],
                    ']' => vec![0x1d],
                    _ => Vec::new(),
                }
            }
        }
    }
}

/// Text typed or pasted: newlines become carriage returns (what Enter sends); `bracketed` wraps
/// it in the paste brackets when the program asked for them.
pub fn text_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_send_the_bytes_a_terminal_sends() {
        assert_eq!(key_bytes(Key::Enter, false), b"\r");
        assert_eq!(key_bytes(Key::Backspace, false), vec![0x7f]);
        assert_eq!(key_bytes(Key::Up, false), b"\x1b[A");
        assert_eq!(key_bytes(Key::Up, true), b"\x1bOA");
        assert_eq!(key_bytes(Key::Delete, false), b"\x1b[3~");
        assert_eq!(key_bytes(Key::PageDown, false), b"\x1b[6~");
        assert_eq!(key_bytes(Key::BackTab, false), b"\x1b[Z");
        assert_eq!(key_bytes(Key::Ctrl('c'), false), vec![3]);
        assert_eq!(key_bytes(Key::Ctrl('D'), false), vec![4]);
        assert_eq!(key_bytes(Key::Ctrl('l'), false), vec![12]);
        assert_eq!(key_bytes(Key::Ctrl('1'), false), Vec::<u8>::new());
    }

    #[test]
    fn pasted_newlines_are_carriage_returns_unless_bracketed() {
        assert_eq!(text_bytes("a\nb\r\nc", false), b"a\rb\rc");
        assert_eq!(text_bytes("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
    }

    #[test]
    fn a_program_output_reaches_the_screen() {
        let command = Command {
            program: PathBuf::from("sh"),
            args: vec!["-c".into(), "printf 'hello from the pty\\n'".into()],
            cwd: std::env::temp_dir(),
            env: Vec::new(),
        };
        let mut term = match Terminal::spawn(&command, 24, 80) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("no pty here: {e}");
                return;
            }
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            term.pump();
            let text = term.screen().contents();
            if text.contains("hello from the pty") {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "the output did not come: {text:?}");
            thread::sleep(std::time::Duration::from_millis(20));
        }
        term.resize(30, 100);
        assert_eq!(term.size(), (30, 100));
    }
}
