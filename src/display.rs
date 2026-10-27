use crate::{options::Options, tracker::Sample};
use std::{
    collections::VecDeque,
    io::{self, Write},
};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::{
    Foundation::{HANDLE, SYSTEMTIME},
    System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, COORD, GetConsoleScreenBufferInfo, GetStdHandle,
        STD_OUTPUT_HANDLE, SetConsoleCursorPosition, WriteConsoleOutputCharacterW,
    },
};

pub struct Display {
    console: Option<HANDLE>,
    start: Option<i16>,
    rows: usize,
}

impl Display {
    pub fn new(plain: bool) -> Self {
        // SAFETY: GetStdHandle borrows the current process's stdout handle.
        let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        let console = (!plain && info(handle).is_ok()).then_some(handle);
        Self {
            console,
            start: Some(0),
            rows: 0,
        }
    }

    pub fn draw(&mut self, text: &str) -> Result<(), String> {
        let Some(handle) = self.console else {
            print!("{text}");
            return io::stdout().flush().map_err(|e| e.to_string());
        };
        let buffer = info(handle)?;
        let width = usize::try_from(buffer.dwSize.X).map_err(|_| "invalid console width")?;
        let height = usize::try_from(buffer.dwSize.Y).map_err(|_| "invalid console height")?;
        if width == 0 || height == 0 {
            return Err("empty console screen buffer".into());
        }

        // Paint complete rows, padding away old digits and rows when output shrinks.
        // Explicit screen-buffer writes do not emit newlines or scroll the console.
        let mut frame = Vec::<u16>::new();
        for line in text.lines() {
            let chars: Vec<u16> = line.encode_utf16().collect();
            let rows = chars.len().max(1).div_ceil(width);
            frame.extend(chars);
            frame.resize(frame.len().div_ceil(width) * width, b' ' as u16);
            if line.is_empty() {
                frame.resize(frame.len() + rows * width, b' ' as u16);
            }
        }
        let content_rows = frame.len() / width;
        let clear_rows = content_rows.max(self.rows).min(height);
        frame.resize(clear_rows * width, b' ' as u16);
        let start = self.start.unwrap_or(buffer.dwCursorPosition.Y).max(0) as usize;
        // Keep the block inside the screen buffer, including after a resize.
        let start = start.min(height.saturating_sub(clear_rows + 1));
        let position = COORD {
            X: 0,
            Y: start as i16,
        };
        let mut written = 0;
        if !frame.is_empty() {
            // SAFETY: frame is readable for frame.len() UTF-16 code units. The
            // coordinates and length fit the console buffer queried above.
            let ok = unsafe {
                WriteConsoleOutputCharacterW(
                    handle,
                    frame.as_ptr(),
                    frame.len() as u32,
                    position,
                    &mut written,
                )
            };
            if ok == 0 || written as usize != frame.len() {
                return Err(console_error("paint console"));
            }
        }
        // Keep the viewport at the top, even as the message area fills up.
        // SAFETY: position is inside the current buffer; handle is borrowed.
        if unsafe { SetConsoleCursorPosition(handle, position) } == 0 {
            return Err(console_error("position console cursor"));
        }
        self.start = Some(start as i16);
        self.rows = content_rows;
        Ok(())
    }

    fn dimensions(&self) -> Result<(usize, usize), String> {
        let buffer = info(self.console.ok_or("stdout is not a console")?)?;
        Ok((
            (buffer.srWindow.Right - buffer.srWindow.Left + 1).max(1) as usize,
            (buffer.srWindow.Bottom - buffer.srWindow.Top + 1).max(1) as usize,
        ))
    }
}

/// One owner for the screen: fixed values above a chronological event tail.
pub struct Dashboard {
    display: Display,
    metrics: String,
    status: String,
    updated: Option<String>,
    messages: VecDeque<String>,
    total_messages: u64,
}

impl Dashboard {
    pub fn new(options: &Options) -> Self {
        Self {
            display: Display::new(options.plain),
            metrics: Sample::default().render(options),
            status: "Waiting for T6 Zombies process".into(),
            updated: None,
            messages: VecDeque::new(),
            total_messages: 0,
        }
    }

    pub fn interactive(&self) -> bool {
        self.display.console.is_some()
    }

    pub fn status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub fn sample(&mut self, sample: &Sample, options: &Options) -> Result<(), String> {
        self.metrics = sample.render(options);
        self.updated = Some(timestamp());
        self.status(if sample.active {
            "Live"
        } else {
            "Attached; waiting for a local server/map"
        });
        if self.interactive() {
            self.paint()
        } else {
            self.display.draw(&format!(
                "Last sample: {} | {}\n{}",
                self.updated.as_deref().unwrap(),
                self.status,
                self.metrics
            ))
        }
    }

    pub fn message(&mut self, level: &str, message: &str) -> Result<(), String> {
        self.message_at(&timestamp(), level, message)
    }

    fn message_at(&mut self, time: &str, level: &str, message: &str) -> Result<(), String> {
        // One event remains one event even when an OS error contains newlines.
        let clean: String = message
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let line = format!("[{time}] {level} {clean}");
        self.total_messages = self.total_messages.saturating_add(1);
        if self.messages.len() == 1000 {
            self.messages.pop_front();
        }
        self.messages.push_back(line.clone());
        if self.interactive() {
            self.paint()
        } else {
            self.display.draw(&(line + "\n"))
        }
    }

    fn paint(&mut self) -> Result<(), String> {
        let (width, height) = self.display.dimensions()?;
        self.display.draw(&self.frame(width, height))
    }

    fn frame(&self, width: usize, height: usize) -> String {
        let width = width.max(1);
        let mut lines = vec![
            format!("T6 Error Tracker v{} (Rust)", env!("CARGO_PKG_VERSION")),
            format!("Status: {}", self.status),
            format!("Last sample: {}", self.updated.as_deref().unwrap_or("--")),
            String::new(),
        ];
        lines.extend(self.metrics.lines().map(str::to_owned));
        lines.push(String::new());
        lines.push(format!("Messages: {} (local time)", self.total_messages));
        // Fixed rows never wrap. Only events wrap, within the lower region.
        for line in &mut lines {
            *line = clip(line, width);
        }
        let available = height.saturating_sub(lines.len());
        let mut tail = VecDeque::new();
        'messages: for message in self.messages.iter().rev() {
            for row in wrap(message, width).into_iter().rev() {
                if tail.len() == available {
                    break 'messages;
                }
                tail.push_front(row);
            }
        }
        lines.extend(tail);
        lines.join("\n") + "\n"
    }
}

fn clip(text: &str, width: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= width
        })
        .collect()
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut units = 0;
    for ch in text.chars() {
        if units + ch.len_utf16() > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
            units = 0;
        }
        row.push(ch);
        units += ch.len_utf16();
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

pub fn timestamp() -> String {
    // SAFETY: GetLocalTime fills this process-owned SYSTEMTIME struct.
    let time = unsafe {
        let mut t: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
    )
}

fn info(handle: HANDLE) -> Result<CONSOLE_SCREEN_BUFFER_INFO, String> {
    // SAFETY: Windows fills a POD struct; no uninitialized data is read on failure.
    unsafe {
        let mut info = std::mem::zeroed();
        if GetConsoleScreenBufferInfo(handle, &mut info) == 0 {
            return Err(console_error("inspect console"));
        }
        Ok(info)
    }
}

fn console_error(action: &str) -> String {
    format!("Cannot {action}: {}", io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Console::{
            AllocConsole, CONSOLE_TEXTMODE_BUFFER, CreateConsoleScreenBuffer, FreeConsole,
            ReadConsoleOutputCharacterW,
        },
    };

    #[test]
    fn native_console_refresh_does_not_append_and_erases_old_text() {
        // Own an inactive screen buffer, so verification never paints over a user's
        // terminal. Allocate a console only when this test process has none.
        // SAFETY: All handles and buffers below belong exclusively to this test.
        unsafe {
            let allocated = AllocConsole() != 0;
            let handle = CreateConsoleScreenBuffer(
                0xC0000000,
                3,
                std::ptr::null(),
                CONSOLE_TEXTMODE_BUFFER,
                std::ptr::null(),
            );
            assert!(
                handle != INVALID_HANDLE_VALUE && !handle.is_null(),
                "{}",
                io::Error::last_os_error()
            );
            struct OwnedConsole(HANDLE, bool);
            impl Drop for OwnedConsole {
                fn drop(&mut self) {
                    unsafe {
                        CloseHandle(self.0);
                        if self.1 {
                            FreeConsole();
                        }
                    }
                }
            }
            let _owned = OwnedConsole(handle, allocated);
            let initial = info(handle).unwrap();
            let mut display = Display {
                console: Some(handle),
                start: Some(0),
                rows: 0,
            };
            display
                .draw("snapshot_ents: 123456789\nold extra row\n")
                .unwrap();
            for _ in 0..100 {
                display.draw("snapshot_ents: 272\n").unwrap();
            }
            let after = info(handle).unwrap();
            assert_eq!(after.dwCursorPosition.Y, 0);
            assert_eq!(after.dwCursorPosition.X, 0);
            assert_eq!(after.dwSize.Y, initial.dwSize.Y);
            let width = after.dwSize.X as usize;
            let mut chars = vec![0u16; width * 2];
            let mut read = 0;
            assert_ne!(
                ReadConsoleOutputCharacterW(
                    handle,
                    chars.as_mut_ptr(),
                    chars.len() as u32,
                    COORD { X: 0, Y: 0 },
                    &mut read
                ),
                0
            );
            let expected: Vec<u16> = "snapshot_ents: 272".encode_utf16().collect();
            assert_eq!(&chars[..expected.len()], expected);
            assert!(chars[expected.len()..].iter().all(|&c| c == b' ' as u16));
            // A block starting near the bottom must also remain in place.
            display.start = Some(after.dwSize.Y - 1);
            display.draw("one\ntwo\nthree\n").unwrap();
            let bottom = info(handle).unwrap().dwCursorPosition.Y;
            display.draw("four\nfive\nsix\n").unwrap();
            assert_eq!(info(handle).unwrap().dwCursorPosition.Y, bottom);

            // Mix metric updates, warnings and reconnects in the same console.
            // Events must never move the metric block or scroll it out of view.
            let options = Options::parse(["-r".into()]).unwrap().unwrap();
            let mut dashboard = Dashboard::new(&options);
            dashboard.display = Display {
                console: Some(handle),
                start: Some(0),
                rows: 0,
            };
            let sample = Sample {
                active: true,
                time: 1000,
                reset: Some(crate::tracker::Reset::new(355937, 6400).unwrap()),
                ..Default::default()
            };
            dashboard.sample(&sample, &options).unwrap();
            for _ in 0..100 {
                dashboard.status("Reconnecting; last readings retained");
                dashboard
                    .message_at("2026-10-27 01:02:03", "WARN", "Sample unavailable")
                    .unwrap();
                dashboard.sample(&sample, &options).unwrap();
            }
            assert_eq!(info(handle).unwrap().dwCursorPosition.Y, 0);
            let (width, height) = dashboard.display.dimensions().unwrap();
            let text = dashboard.frame(width, height);
            let lines: Vec<_> = text.lines().collect();
            assert!(lines[4].starts_with("level_time:"));
            assert!(lines[5].starts_with("snapshot_ents:"));
            assert!(text.contains("Messages: 100 (local time)"));
            assert!(text.contains("[2026-10-27 01:02:03] WARN Sample unavailable"));
            let mut actual = vec![0u16; info(handle).unwrap().dwSize.X as usize * height];
            assert_ne!(
                ReadConsoleOutputCharacterW(
                    handle,
                    actual.as_mut_ptr(),
                    actual.len() as u32,
                    COORD { X: 0, Y: 0 },
                    &mut read
                ),
                0
            );
            let actual = String::from_utf16_lossy(&actual);
            assert!(actual.contains("snapshot_ents:"));
            assert!(actual.contains("Messages: 100 (local time)"));
            assert!(actual.contains("[2026-10-27 01:02:03] WARN Sample unavailable"));
        }
    }

    #[test]
    fn event_tail_keeps_metrics_fixed_at_different_window_sizes() {
        let options = Options::parse(["-r".into()]).unwrap().unwrap();
        let mut dashboard = Dashboard::new(&options);
        dashboard.messages = (0..100)
            .map(|i| format!("[2026-10-27 01:02:03] WARN event {i}"))
            .collect();
        dashboard.total_messages = 100;
        for (width, height) in [(80, 25), (40, 20), (120, 40)] {
            let frame = dashboard.frame(width, height);
            let lines: Vec<_> = frame.lines().collect();
            assert!(lines.len() <= height);
            assert!(lines[4].starts_with("level_time:"));
            assert!(lines[5].starts_with("snapshot_ents:"));
            assert!(frame.contains("event 99"));
            assert!(!frame.contains("event 0\n"));
            assert!(
                lines
                    .iter()
                    .all(|line| line.encode_utf16().count() <= width)
            );
        }
    }
}
