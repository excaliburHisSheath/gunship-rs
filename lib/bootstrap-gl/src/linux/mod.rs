use std::ffi::CString;
use std::mem;
use std::ptr;

use gl;
use super::x11::glx;
// use super::x11::xlib;

use window::Window;

pub type Context = glx::GLXContext;

pub unsafe fn init(_window: &Window) {
    println!("gl::init() is not implemented on linux");
}

pub unsafe fn create_context(_window: &Window) -> GLContext {
    set_proc_loader();
    //
    // context

    ptr::null_mut()
}

pub unsafe fn set_proc_loader() {
    // provide method for loading functions
    gl::load_with(|s| {
        let string = CString::new(s);
        mem::transmute(glx::glXGetProcAddress(mem::transmute(string.unwrap().as_ptr())))
    });
}

pub unsafe fn swap_buffers(window: &Window) {
    glx::glXSwapBuffers(window.display, window.window);
}
