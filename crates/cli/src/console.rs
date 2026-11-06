//! Where the game's text goes on Windows.
//!
//! The release binary is linked as a windowed program, so double-clicking it
//! opens the game and nothing else: no black console window in front of the
//! pitch. A windowed program is born without a console at all, which is the
//! point, but it leaves the text commands (`7on7 sim`, `7on7 rules`, ...) with
//! nowhere to write.
//!
//! So on the way in the program borrows the console of whoever started it, if
//! there is one: run from `cmd` and the output appears there as it always did;
//! started from Explorer and there is nobody to borrow from, so the text is
//! pointed at the null device and the game opens. Printing never decides whether
//! the game runs.

/// Put this process's text on the terminal that started it, or on nothing.
pub fn adopt() {
    imp::adopt();
}

#[cfg(not(windows))]
mod imp {
    /// Every other platform hands a program its terminal already.
    pub fn adopt() {}
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::os::windows::io::IntoRawHandle;

    // The three calls needed to find or replace the standard handles. Declared
    // here rather than pulled in with a crate: they are in kernel32, which every
    // Windows program already links.
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process: u32) -> i32;
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
    }

    /// `ATTACH_PARENT_PROCESS`: the console of the shell that launched us.
    const PARENT: u32 = 0xFFFF_FFFF;
    const STD_OUTPUT: u32 = -11i32 as u32;
    const STD_ERROR: u32 = -12i32 as u32;

    pub fn adopt() {
        // A console that came with the launch (a build made with the `console`
        // feature, or an older one) is already where the text belongs.
        unsafe {
            if usable(GetStdHandle(STD_OUTPUT)) && usable(GetStdHandle(STD_ERROR)) {
                return;
            }
            let attached = AttachConsole(PARENT) != 0;
            for which in [STD_OUTPUT, STD_ERROR] {
                if usable(GetStdHandle(which)) {
                    // Redirection (`7on7 teams > teams.txt`) arrived with the
                    // launch: keep it rather than pointing at the screen.
                    continue;
                }
                // `CONOUT$` is the borrowed console's screen; `NUL` is the null
                // device, which is what a double-click gets. Either way the
                // handle is valid, so a stray `println!` is never fatal.
                let device = if attached { "CONOUT$" } else { "NUL" };
                if let Ok(file) = OpenOptions::new().write(true).open(device) {
                    // The handle outlives this function, so the file it came
                    // from is deliberately not closed.
                    SetStdHandle(which, file.into_raw_handle() as *mut c_void);
                }
            }
        }
    }

    /// A handle a write can actually go to.
    fn usable(handle: *mut c_void) -> bool {
        !handle.is_null() && handle as isize != -1
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn adopting_a_console_leaves_a_writable_stdout_behind() {
        super::adopt();
        // The point of the null device: writing never fails, console or not.
        use std::io::Write;
        assert!(writeln!(std::io::stdout(), "").is_ok());
    }
}
