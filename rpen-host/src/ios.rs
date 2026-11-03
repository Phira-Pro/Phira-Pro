use anyhow::Result;
use std::ffi::{c_char, CString};
extern "C" {
    fn rpen_ios_open(url: *const c_char);
    fn rpen_ios_visible(visible: bool);
    fn rpen_ios_close();
}
pub fn open(url: &str) -> Result<()> {
    let url = CString::new(url)?;
    unsafe {
        rpen_ios_open(url.as_ptr());
    }
    Ok(())
}
pub fn visible(visible: bool) {
    unsafe {
        rpen_ios_visible(visible);
    }
}
pub fn close() {
    unsafe {
        rpen_ios_close();
    }
}
