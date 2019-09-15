#![feature(asm)]
extern crate exvm;

use exvm::gc::GC;
use exvm::heap::*;

fn main() {
    init_page_size();
    let mut h = Heap::new(page_size() as _);
    h.needs_gc = GCType::NewSpace;
    let mut gc = GC::new(h.val);
    let s = "Hello!";
    let num = HString::new(&mut h, Tenure::New, s.len(), Some(s));
    gc.collect_garbage(&num as *const _ as *mut _);
}
