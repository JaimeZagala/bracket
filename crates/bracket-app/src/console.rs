//! Printing `--help` and `--version`. Release builds are GUI programs (no console of their
//! own), so the text goes to the console Bracket was started from, or to a message box when
//! there is none (started from Explorer or a shortcut).

use std::io::Write;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
};

/// Where standard output goes.
enum Output {
    /// A file, a pipe, or a debug build's own console.
    Ready,
    /// The console of the shell that started Bracket, which has usually printed its prompt
    /// already.
    ParentConsole,
    Nowhere,
}

/// Shows `text` to whoever started Bracket.
pub fn show(text: &str) {
    let mut stdout = std::io::stdout();
    match output() {
        Output::Ready => {
            let _ = writeln!(stdout, "{text}");
        }
        Output::ParentConsole => {
            // Start on a fresh line, after the prompt.
            let _ = writeln!(stdout, "\n{text}");
        }
        Output::Nowhere => bracket_ui::show_message(text),
    }
    let _ = stdout.flush();
}

#[allow(unsafe_code)]
fn output() -> Output {
    // SAFETY: GetStdHandle and AttachConsole take plain integer arguments, share no memory with
    // the program and only read or change this process's standard handles. The returned handle
    // is only compared, never closed.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            Output::Ready
        } else if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            Output::ParentConsole
        } else {
            Output::Nowhere
        }
    }
}
