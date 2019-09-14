#![feature(asm)]
extern crate exvm;

use exvm::gc::GC;
use exvm::heap::*;

fn main() {
    let mut h = Heap::new(4096 * 2);
    let mut gc = GC::new(h.val);
    let num = HNumber::newf(&mut h, Tenure::New, 3.14);
    let _num2 = HObject::new_empty(&mut h, 2);
    let _num2 = HNumber::newf(&mut h, Tenure::New, 3.14);
    gc.collect_garbage(&num as *const _ as *mut _);
}
