#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    clash_nyanpasu_lib::run().unwrap();
}
