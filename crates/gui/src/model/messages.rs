//! Item 20 (DECISIONS AG112-AG114): the one store behind the toasts and the Messages log. A status-line note, a
//! routine result and the picker note all go through here; nothing keeps a second copy. In memory only.
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

/// The Messages tab keeps this many entries; the oldest go first.
pub const LOG_CAP: usize = 500;
/// Toasts on screen at once; the rest wait in a queue.
pub const MAX_SHOWN: usize = 3;
/// Toasts waiting for a place; the oldest are dropped past this (the log keeps them).
pub const MAX_QUEUED: usize = 5;
/// How long a toast stays, and a warning or error.
pub const FADE: Duration = Duration::from_secs(4);
pub const FADE_WARN: Duration = Duration::from_secs(8);
/// A toast with a button (Note FB5) stays long enough to be answered: still frozen on hover.
pub const FADE_ACTION: Duration = Duration::from_secs(20);
/// The last part of a toast's time fades it out (on egui time, `ui::messages`; Note CF2).
pub const FADE_OUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "Info",
            Severity::Warning => "Warning",
            Severity::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Local `HH:MM:SS` when it was logged.
    pub time: String,
    pub severity: Severity,
    pub text: String,
}

/// What a toast's button does (Note FB5): the toast draws it as `Undo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastAction {
    /// Takes back the crash-edit restore and deletes the `.recovery` files (`AppModel::undo_crash_restore`).
    UndoCrashRestore,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub severity: Severity,
    pub text: String,
    pub action: Option<ToastAction>,
    /// Time left on screen; frozen while the pointer is over the toast.
    pub left: Duration,
}

pub struct Messages {
    log: VecDeque<Entry>,
    unread: usize,
    open: bool,
    shown: Vec<Toast>,
    queue: VecDeque<Toast>,
    next_id: u64,
    /// The clock of the last `tick`; `None` while no toast is on screen, so a new toast starts with its full time.
    last_tick: Option<Instant>,
    /// The toasts hovered at the last `tick`. A resting pointer sends no frames, so the frame it leaves on has the
    /// whole rest as its `dt`; that frame charges these nothing.
    hovered_last: HashSet<u64>,
    /// Gives the entry its time text; tests replace it.
    pub clock: fn() -> String,
}

impl Default for Messages {
    fn default() -> Messages {
        Messages {
            log: VecDeque::new(),
            unread: 0,
            open: false,
            shown: vec![],
            queue: VecDeque::new(),
            next_id: 0,
            last_tick: None,
            hovered_last: HashSet::new(),
            clock: local_time,
        }
    }
}

impl Messages {
    /// Only the log (a status-line note: the line itself stays).
    pub fn log(&mut self, severity: Severity, text: impl Into<String>) {
        self.log.push_back(Entry {
            time: (self.clock)(),
            severity,
            text: text.into(),
        });
        if self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
        if !self.open {
            self.unread = (self.unread + 1).min(self.log.len());
        }
    }

    /// The log and a toast (a routine result).
    pub fn notify(&mut self, severity: Severity, text: impl Into<String>) {
        self.notify_with(severity, text, None);
    }

    /// [`Self::notify`] with a button on the toast.
    pub fn notify_with(
        &mut self,
        severity: Severity,
        text: impl Into<String>,
        action: Option<ToastAction>,
    ) {
        let text = text.into();
        self.log(severity, text.clone());
        self.next_id += 1;
        // A newer notice of the same kind ("Undid", "Saved", "Export finished") replaces the one still waiting.
        let kind = |t: &str| t.split(':').next().unwrap_or_default().to_string();
        let k = kind(&text);
        self.queue.retain(|q| kind(&q.text) != k);
        while self.queue.len() >= MAX_QUEUED {
            self.queue.pop_front();
        }
        self.queue.push_back(Toast {
            id: self.next_id,
            severity,
            text,
            action,
            left: if action.is_some() {
                FADE_ACTION
            } else if severity == Severity::Info {
                FADE
            } else {
                FADE_WARN
            },
        });
        self.fill();
    }

    fn fill(&mut self) {
        while self.shown.len() < MAX_SHOWN {
            let Some(t) = self.queue.pop_front() else {
                break;
            };
            self.shown.push(t);
        }
    }

    /// Advances the toasts to `now`. A toast in `hovered`, or hovered at the last tick, keeps its time. Returns true while any toast is on screen
    /// or waiting (the caller keeps frames coming).
    pub fn tick(&mut self, now: Instant, hovered: &HashSet<u64>) -> bool {
        if let Some(last) = self.last_tick {
            let dt = now.saturating_duration_since(last);
            for t in self.shown.iter_mut() {
                if !hovered.contains(&t.id) && !self.hovered_last.contains(&t.id) {
                    t.left = t.left.saturating_sub(dt);
                }
            }
            self.shown.retain(|t| !t.left.is_zero());
        }
        self.hovered_last.clone_from(hovered);
        self.fill();
        self.last_tick = (!self.shown.is_empty()).then_some(now);
        self.last_tick.is_some()
    }

    /// Takes a toast off the screen (its button was clicked); the log keeps the entry.
    pub fn dismiss(&mut self, id: u64) {
        self.shown.retain(|t| t.id != id);
    }

    pub fn toasts(&self) -> &[Toast] {
        &self.shown
    }

    /// Toasts waiting for a free place.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &Entry> {
        self.log.iter()
    }

    pub fn len(&self) -> usize {
        self.log.len()
    }

    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }

    /// Entries logged while the Messages tab was not showing.
    pub fn unread(&self) -> usize {
        self.unread
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opening reads everything; so does anything logged while it is open.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        if open {
            self.unread = 0;
        }
    }

    /// The Clear button: the log goes, a toast on screen finishes on its own.
    pub fn clear(&mut self) {
        self.log.clear();
        self.unread = 0;
    }
}

#[cfg(windows)]
fn local_time() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: GetLocalTime only writes the struct it is given.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
}

/// shortcut: UTC off Windows (the GUI ships for Windows only); use the platform's local time when it ships elsewhere.
#[cfg(not(windows))]
fn local_time() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        % 86_400;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}
