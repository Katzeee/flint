//! Operating-system entry points and loaded-module access.

mod windows;

pub(crate) use windows::{export, find_module, list_modules, module_with_export};
