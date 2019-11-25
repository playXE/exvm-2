extern crate exvm;

use assembler::Assembler;
use assembler_x64::*;
use capstone::prelude::*;
use constants_x64::*;
use exvm::gc::GC;
use exvm::heap::*;
use generic::*;
use jazz_jit::*;

extern "C" {
    fn puts(_: *mut u8);
}

fn main() {
    init_page_size();
    let mut h = Heap::new(page_size() as _);
    h.needs_gc = GCType::NewSpace;
    let mut gc = GC::new(h.val);
    let s = "Hello!";
    let s2 = "Hello again";
    let mut first = HString::new(&mut h, Tenure::New, s.len(), Some(s));
    let mut another = HString::new(&mut h, Tenure::New, s2.len(), Some("Hello again"));
    //let x = &another;
    println!("{:p}", another);
    gc.collect_garbage(&first as *const _ as *mut _);
    unsafe {
        puts(HString::value(h.val, first));
        puts(HString::value(h.val, another));
    }
    println!("{:p}", another);
    another = std::ptr::null_mut();
    gc.collect_garbage(&first as *const _ as *mut _);
    println!("{:p}", another);
    /*let mut asm = Assembler::new();

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
    println!("{}", fun(2.6));*/
}
