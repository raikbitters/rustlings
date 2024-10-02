use crossterm::{
    cursor::MoveTo,
    style::{Attribute, Color, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
    Command, QueueableCommand,
};
use std::{
    fmt, fs,
    io::{self, BufRead, StdoutLock, Write},
};

pub const PROGRESS_FAILED_COLOR: Color = Color::Red;
pub const PROGRESS_SUCCESS_COLOR: Color = Color::Green;
pub const PROGRESS_PENDING_COLOR: Color = Color::Blue;

pub struct MaxLenWriter<'a, 'b> {
    pub stdout: &'a mut StdoutLock<'b>,
    len: usize,
    max_len: usize,
}

impl<'a, 'b> MaxLenWriter<'a, 'b> {
    #[inline]
    pub fn new(stdout: &'a mut StdoutLock<'b>, max_len: usize) -> Self {
        Self {
            stdout,
            len: 0,
            max_len,
        }
    }

    // Additional is for emojis that take more space.
    #[inline]
    pub fn add_to_len(&mut self, additional: usize) {
        self.len += additional;
    }
}

pub trait CountedWrite<'a> {
    fn write_ascii(&mut self, ascii: &[u8]) -> io::Result<()>;
    fn write_str(&mut self, unicode: &str) -> io::Result<()>;
    fn stdout(&mut self) -> &mut StdoutLock<'a>;
}

impl<'a, 'b> CountedWrite<'b> for MaxLenWriter<'a, 'b> {
    fn write_ascii(&mut self, ascii: &[u8]) -> io::Result<()> {
        let n = ascii.len().min(self.max_len.saturating_sub(self.len));
        if n > 0 {
            self.stdout.write_all(&ascii[..n])?;
            self.len += n;
        }
        Ok(())
    }

    fn write_str(&mut self, unicode: &str) -> io::Result<()> {
        if let Some((ind, c)) = unicode
            .char_indices()
            .take(self.max_len.saturating_sub(self.len))
            .last()
        {
            self.stdout
                .write_all(&unicode.as_bytes()[..ind + c.len_utf8()])?;
            self.len += ind + 1;
        }

        Ok(())
    }

    #[inline]
    fn stdout(&mut self) -> &mut StdoutLock<'b> {
        self.stdout
    }
}

impl<'a> CountedWrite<'a> for StdoutLock<'a> {
    #[inline]
    fn write_ascii(&mut self, ascii: &[u8]) -> io::Result<()> {
        self.write_all(ascii)
    }

    #[inline]
    fn write_str(&mut self, unicode: &str) -> io::Result<()> {
        self.write_all(unicode.as_bytes())
    }

    #[inline]
    fn stdout(&mut self) -> &mut StdoutLock<'a> {
        self
    }
}

/// Simple terminal progress bar
pub fn progress_bar<'a>(
    writer: &mut impl CountedWrite<'a>,
    progress: u16,
    total: u16,
    line_width: u16,
) -> io::Result<()> {
    progress_bar_with_success(writer, 0, 0, progress, total, line_width)
}
/// Terminal progress bar with three states (pending + failed + success)
pub fn progress_bar_with_success<'a>(
    writer: &mut impl CountedWrite<'a>,
    pending: u16,
    failed: u16,
    success: u16,
    total: u16,
    line_width: u16,
) -> io::Result<()> {
    debug_assert!(total < 1000);
    debug_assert!((pending + failed + success) <= total);

    const PREFIX: &[u8] = b"Progress: [";
    const PREFIX_WIDTH: u16 = PREFIX.len() as u16;
    const POSTFIX_WIDTH: u16 = "] xxx/xxx".len() as u16;
    const WRAPPER_WIDTH: u16 = PREFIX_WIDTH + POSTFIX_WIDTH;
    const MIN_LINE_WIDTH: u16 = WRAPPER_WIDTH + 4;

    if line_width < MIN_LINE_WIDTH {
        writer.write_ascii(b"Progress: ")?;
        // Integers are in ASCII.
        return writer.write_ascii(format!("{}/{total}", failed + success).as_bytes());
    }

    let stdout = writer.stdout();
    stdout.write_all(PREFIX)?;

    let width = line_width - WRAPPER_WIDTH;
    let mut failed_end = (width * failed) / total;
    let mut success_end = (width * (failed + success)) / total;
    let mut pending_end = (width * (failed + success + pending)) / total;

    // In case the range boundaries overlap, "pending" has priority over both
    // "failed" and "success" (don't show the bar as "complete" when we are
    // still checking some things).
    // "Failed" has priority over "success" (don't show 100% success if we
    // have some failures, at the risk of showing 100% failures even with
    // a few successes).
    //
    // "Failed" already has priority over "success" because it's displayed
    // first. But "pending" is last so we need to fix "success"/"failed".
    if pending > 0 {
        pending_end = pending_end.max(1);
        if pending_end == success_end {
            success_end -= 1;
        }
        if pending_end == failed_end {
            failed_end -= 1;
        }

        // This will replace the last character of the "pending" range with
        // the arrow char ('>'). This ensures that even if the progress bar
        // is filled (everything either done or pending), we'll still see
        // the '>' as long as we are not fully done.
        pending_end -= 1;
    }

    if failed > 0 {
        stdout.queue(SetForegroundColor(PROGRESS_FAILED_COLOR))?;
        for _ in 0..failed_end {
            stdout.write_all(b"#")?;
        }
    }

    stdout.queue(SetForegroundColor(PROGRESS_SUCCESS_COLOR))?;
    for _ in failed_end..success_end {
        stdout.write_all(b"#")?;
    }

    if pending > 0 {
        stdout.queue(SetForegroundColor(PROGRESS_PENDING_COLOR))?;

        for _ in success_end..pending_end {
            stdout.write_all(b"#")?;
        }
    }

    if pending_end < width {
        stdout.write_all(b">")?;
    }

    let width_minus_filled = width - pending_end;
    if width_minus_filled > 1 {
        let red_part_width = width_minus_filled - 1;
        stdout.queue(SetForegroundColor(Color::Red))?;
        for _ in 0..red_part_width {
            stdout.write_all(b"-")?;
        }
    }

    stdout.queue(SetForegroundColor(Color::Reset))?;

    write!(stdout, "] {:>3}/{}", failed + success, total)
}

pub fn clear_terminal(stdout: &mut StdoutLock) -> io::Result<()> {
    stdout
        .queue(MoveTo(0, 0))?
        .queue(Clear(ClearType::All))?
        .queue(Clear(ClearType::Purge))
        .map(|_| ())
}

pub fn press_enter_prompt(stdout: &mut StdoutLock) -> io::Result<()> {
    stdout.flush()?;
    io::stdin().lock().read_until(b'\n', &mut Vec::new())?;
    stdout.write_all(b"\n")
}

/// Canonicalize, convert to string and remove verbatim part on Windows.
pub fn canonicalize(path: &str) -> Option<String> {
    fs::canonicalize(path)
        .ok()?
        .into_os_string()
        .into_string()
        .ok()
        .map(|mut path| {
            // Windows itself can't handle its verbatim paths.
            if cfg!(windows) && path.as_bytes().starts_with(br"\\?\") {
                path.drain(..4);
            }

            path
        })
}

pub fn terminal_file_link<'a>(
    writer: &mut impl CountedWrite<'a>,
    path: &str,
    canonical_path: &str,
    color: Color,
) -> io::Result<()> {
    writer
        .stdout()
        .queue(SetForegroundColor(color))?
        .queue(SetAttribute(Attribute::Underlined))?;
    writer.stdout().write_all(b"\x1b]8;;file://")?;
    writer.stdout().write_all(canonical_path.as_bytes())?;
    writer.stdout().write_all(b"\x1b\\")?;
    // Only this part is visible.
    writer.write_str(path)?;
    writer.stdout().write_all(b"\x1b]8;;\x1b\\")?;
    writer
        .stdout()
        .queue(SetForegroundColor(Color::Reset))?
        .queue(SetAttribute(Attribute::NoUnderline))?;

    Ok(())
}

pub fn write_ansi(output: &mut Vec<u8>, command: impl Command) {
    struct FmtWriter<'a>(&'a mut Vec<u8>);

    impl fmt::Write for FmtWriter<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0.extend_from_slice(s.as_bytes());
            Ok(())
        }
    }

    let _ = command.write_ansi(&mut FmtWriter(output));
}
