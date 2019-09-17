#![feature(asm)]
extern crate exvm;

use assembler::Assembler;
use assembler_x64::*;
use capstone::prelude::*;
use constants_x64::*;
use exvm::gc::GC;
use exvm::heap::*;
use generic::*;
use jazz_jit::*;
fn main() {
    init_page_size();
    let mut h = Heap::new(page_size() as _);
    h.needs_gc = GCType::NewSpace;
    /*let mut gc = GC::new(h.val);
    let s = "Hello!";
    let num = HString::new(&mut h, Tenure::New, s.len(), Some(s));
    gc.collect_garbage(&num as *const _ as *mut _);*/

    let mut asm = Assembler::new();

    roundsd(&mut asm, XMM0, XMM0, RoundMode::Nearest);
    asm.ret();
    let cs = Capstone::new()
        .x86()
        .mode(arch::x86::ArchMode::Mode64)
        .syntax(arch::x86::ArchSyntax::Att)
        .detail(true)
        .build()
        .unwrap();
    for op in asm.data().iter() {
        print!("0x{:x} ", op);
    }
    println!();
    let ins = cs.disasm_all(asm.data(), 0x0);
    for ins in ins.unwrap().iter() {
        println!("{}", ins);
    }

    let mem = jazz_jit::get_executable_memory(&asm);
    let fun: fn(f64) -> f64 = unsafe { std::mem::transmute(mem.ptr()) };
    println!("{}", fun(2.6));
}
