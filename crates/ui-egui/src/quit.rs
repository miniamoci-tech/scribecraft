//! Leaving the window the platform's own way.
//!
//! On macOS an eframe app must not leave by closing its window. eframe drops the window while
//! AppKit's run loop is still running, and in the next display cycle AppKit's Touch Bar finder
//! tries to remove a KVO observer from the view that is gone
//! (`-[_NSTouchBarFinderObservation invalidate]`, "Cannot remove an observer … because it is
//! not registered as an observer"). That is an uncaught Objective-C exception, so the process
//! aborts on quit — seen on this Mac (MacBookPro17,1, macOS 26) and in PdfCraft (#1833).
//! `-[NSApplication terminate:]`, which is what ⌘Q does, ends the process before another
//! display cycle runs.
//!
//! [`take_over_close`] is called from [`eframe::App::logic`], so it also runs while the window
//! is minimized. When the root window was asked to close it cancels the close and schedules
//! the platform quit. The window stays open for the moment it takes to quit, and eframe's
//! [`eframe::App::on_exit`] still runs from winit's `applicationWillTerminate:`, so the job
//! list is saved.
//!
//! Elsewhere the window closes as it always did: nothing here changes Windows, Linux or
//! FreeBSD.

use eframe::egui::{self, ViewportCommand, ViewportId};

/// Quit through a path of this platform's own, where leaving by closing the window is not safe.
///
/// macOS: schedules `-[NSApplication terminate:]` and returns `true`; the caller keeps its
/// window open and the process ends shortly after, once the current event has been handled.
/// Other platforms: does nothing and returns `false`.
pub(crate) fn quit() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::terminate_later();
        true
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Call once per frame at the end of [`eframe::App::logic`].
///
/// When the root window was asked to close (the close button, a `ViewportCommand::Close`, the
/// operating system) and this platform has its own quit, cancels the close and quits that way
/// instead. Returns whether it took the close over. `false` means there was no close request or
/// the platform has no quit of its own, and eframe goes on as it would without this call.
///
/// eframe reads the frame's viewport commands only after the frame, so a `CancelClose` that the
/// app sent itself in `ui` cannot be seen from `logic`: an app with unsaved work must keep the
/// window open with [`egui::ViewportCommand::CancelClose`] and only then let this run, or keep
/// its prompt in `logic`. ScribeCraft saves every edit as it happens and has no such prompt.
pub(crate) fn take_over_close(ctx: &egui::Context) -> bool {
    take_over_close_with(ctx, quit)
}

/// [`take_over_close`] with the platform quit passed in, so the decision can be tested without
/// a window: `quit` returns whether it quits.
fn take_over_close_with(ctx: &egui::Context, quit: impl FnOnce() -> bool) -> bool {
    if ctx.viewport_id() != ViewportId::ROOT || !ctx.input(|i| i.viewport().close_requested()) {
        return false;
    }
    if !quit() {
        return false;
    }
    ctx.send_viewport_cmd(ViewportCommand::CancelClose);
    true
}

#[cfg(target_os = "macos")]
mod macos {
    //! `-[NSApplication terminate:]`, scheduled on the main queue. Safe objc2 calls only.

    use dispatch2::DispatchQueue;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    /// Send `terminate:` from the main queue rather than from the caller. The caller is inside
    /// winit's event handler, and `terminate:` makes winit deliver its exit event, which it
    /// cannot do while another event is being handled. From the main queue winit is idle: its
    /// `applicationWillTerminate:` closes the window and runs the app's exit handling (eframe
    /// saves the job list), and AppKit ends the process.
    pub(super) fn terminate_later() {
        DispatchQueue::main().exec_async(|| {
            // The main queue runs on the main thread, so there is always a marker.
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm).terminate(None);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use egui::{RawInput, ViewportEvent};

    use super::*;

    /// One frame of a window, with its close button pressed or not. Returns whether the close
    /// was taken over, how often the quit ran and the window's commands for that frame.
    fn frame(close: bool, quits: bool) -> (bool, u32, Vec<ViewportCommand>) {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        if close {
            input.viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
        }
        let (taken, quit_calls) = (Cell::new(false), Cell::new(0));
        let mut out = ctx.run_ui(input, |ui| {
            taken.set(take_over_close_with(ui.ctx(), || {
                quit_calls.set(quit_calls.get() + 1);
                quits
            }));
        });
        out.textures_delta.clear(); // not painted
        let commands = out.viewport_output.remove(&ViewportId::ROOT).map(|o| o.commands).unwrap_or_default();
        (taken.get(), quit_calls.get(), commands)
    }

    fn cancels(commands: &[ViewportCommand]) -> bool {
        commands.contains(&ViewportCommand::CancelClose)
    }

    #[test]
    fn a_close_request_quits_and_keeps_the_window() {
        let (taken, quit_calls, commands) = frame(true, true);
        assert!(taken);
        assert_eq!(quit_calls, 1);
        assert!(cancels(&commands), "the window stays for the quit to close it");
    }

    #[test]
    fn nothing_happens_without_a_close_request() {
        let (taken, quit_calls, commands) = frame(false, true);
        assert!(!taken);
        assert_eq!(quit_calls, 0);
        assert!(!cancels(&commands));
    }

    /// Windows, Linux and FreeBSD have no quit of their own: the window closes as before.
    #[test]
    fn without_a_platform_quit_the_window_closes_as_usual() {
        let (taken, quit_calls, commands) = frame(true, false);
        assert!(!taken);
        assert_eq!(quit_calls, 1);
        assert!(!cancels(&commands), "eframe closes the window itself");
    }

    /// Only the root window is ours to take over; a child viewport's close is eframe's.
    #[test]
    fn a_child_viewport_close_is_left_to_eframe() {
        let ctx = egui::Context::default();
        let child = ViewportId::from_hash_of("child");
        let mut input = RawInput { viewport_id: child, ..RawInput::default() };
        input.viewports.entry(child).or_default().parent = Some(ViewportId::ROOT);
        input.viewports.entry(child).or_default().events.push(ViewportEvent::Close);
        let quit_calls = Cell::new(0);
        let mut out = ctx.run_ui(input, |ui| {
            take_over_close_with(ui.ctx(), || {
                quit_calls.set(quit_calls.get() + 1);
                true
            });
        });
        out.textures_delta.clear(); // not painted
        assert_eq!(quit_calls.get(), 0);
    }
}
