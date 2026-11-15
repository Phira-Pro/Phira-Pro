use anyhow::{ensure, Result};
use std::{mem, ptr};
use winapi::{
    shared::minwindef::HGLOBAL,
    um::{
        winbase::{GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        winuser::{CloseClipboard, EmptyClipboard, GetActiveWindow, OpenClipboard, SetClipboardData, CF_UNICODETEXT},
    },
};

struct Clipboard;

impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

struct ClipboardText(HGLOBAL);

impl Drop for ClipboardText {
    fn drop(&mut self) {
        unsafe { GlobalFree(self.0) };
    }
}

pub(super) fn copy(text: &str) -> Result<()> {
    let text: Vec<u16> = text.encode_utf16().map(|ch| if ch == 0 { 0xfffd } else { ch }).chain([0]).collect();
    // The system takes ownership of the unlocked allocation only on success.
    unsafe {
        let owner = GetActiveWindow();
        ensure!(!owner.is_null(), "No active window for the Windows clipboard");
        ensure!(OpenClipboard(owner) != 0, "Could not open the Windows clipboard");
        let _clipboard = Clipboard;
        let handle = GlobalAlloc(GMEM_MOVEABLE, mem::size_of_val(text.as_slice()));
        ensure!(!handle.is_null(), "Could not allocate clipboard text");
        let data = ClipboardText(handle);
        let out = GlobalLock(handle).cast::<u16>();
        ensure!(!out.is_null(), "Could not lock clipboard text");
        ptr::copy_nonoverlapping(text.as_ptr(), out, text.len());
        GlobalUnlock(handle);
        ensure!(EmptyClipboard() != 0, "Could not empty the Windows clipboard");
        ensure!(!SetClipboardData(CF_UNICODETEXT, handle).is_null(), "Could not write clipboard text");
        mem::forget(data);
    }
    Ok(())
}
