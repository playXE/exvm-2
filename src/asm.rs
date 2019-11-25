use crate::heap::*;
use generic::*;
use jazz_jit::assembler::*;
use jazz_jit::assembler_x64::*;
use jazz_jit::constants_x64::*;
use jazz_jit::*;
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct Spill {
    pub src: Register,
    pub index: i32,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum RelocSize {
    Byte,
    Word,
    Long,
    Quad,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum RelocType {
    Absoulte,
    Value,
    Relative,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct RelocationInfo {
    pub offset: u32,
    pub target: u32,
    pub ty: RelocType,
    pub size: RelocSize,
    pub notify_gc: bool,
}

impl RelocationInfo {
    pub const fn new(ty: RelocType, size: RelocSize, offset: u32) -> Self {
        Self {
            offset,
            ty,
            size,
            target: 0,
            notify_gc: false,
        }
    }

    pub unsafe fn relocate(&self, _: &mut Heap, buffer: &mut [u8]) {
        let mut addr: u64;
        match self.ty {
            RelocType::Absoulte => addr = buffer.as_ptr().offset(self.target as _) as u64,
            RelocType::Value => addr = self.target as _,
            _ => addr = self.target as u64 - self.offset as u64,
        };
        match self.size {
            RelocSize::Byte => addr -= 1,
            RelocSize::Word => addr -= 2,
            RelocSize::Long => addr -= 4,
            RelocSize::Quad => addr -= 8,
        }

        match self.size {
            RelocSize::Byte => {
                *(buffer.as_mut_ptr().offset(self.offset as _)) = addr as u8;
            }
            RelocSize::Word => {
                *(buffer.as_mut_ptr().offset(self.offset as _) as *mut u16) = addr as u16;
            }
            RelocSize::Long => {
                *(buffer.as_mut_ptr().offset(self.offset as _) as *mut u32) = addr as u32;
            }
            RelocSize::Quad => {
                *(buffer.as_mut_ptr().offset(self.offset as _) as *mut u64) = addr;
            }
        }
    }
}

pub const CONTEXT_REG: Register = RSI;
pub const ROOT_REG: Register = RDI;
pub const SCRATCH: Register = R14;

use std::cell::RefCell;
use std::rc::Rc;
pub struct Masm {
    relocation_info: Vec<Rc<RefCell<RelocationInfo>>>,
    asm: Assembler,
    stubs: std::collections::HashMap<&'static str, *const u8>,
}

impl Masm {
    pub fn new() -> Self {
        Self {
            relocation_info: vec![],
            asm: Assembler::new(),
            stubs: std::collections::HashMap::new(),
        }
    }
    pub unsafe fn prologue(&mut self) {
        self.push(RBP);
        (**self).mov(true, RSP, RBP);
    }

    pub unsafe fn epilogue(&mut self, args: u16) {
        (**self).mov(true, RBP, RSP);
        self.pop(RBP);
        emit_retq_imm(&mut self.asm, args * 8);
    }
    pub unsafe fn relocate(&mut self, heap: &mut Heap) {
        for item in self.relocation_info.iter().cloned() {
            item.borrow().relocate(heap, &mut self.asm.data);
        }
    }
    pub unsafe fn push(&mut self, r: Register) {
        emit_pushq_reg(&mut self.asm, r);
    }
    pub unsafe fn pop(&mut self, r: Register) {
        emit_popq_reg(&mut self.asm, r);
    }
    pub unsafe fn pushad(&mut self) {
        self.push(RAX);
        self.push(RBX);
        self.push(RCX);
        self.push(RDX);
        self.push(R8);
        self.push(R9);
        self.push(R10);
        self.push(R11);
        self.push(R12);
        self.push(R13);
        self.push(ROOT_REG);
        self.push(CONTEXT_REG);
    }

    pub unsafe fn popad(&mut self, r: Register) {
        self.preserve_pop(CONTEXT_REG, r);
        self.preserve_pop(ROOT_REG, r);
        self.preserve_pop(R13, r);
        self.preserve_pop(R12, r);
        self.preserve_pop(R11, r);
        self.preserve_pop(R10, r);
        self.preserve_pop(R9, r);
        self.preserve_pop(R8, r);
        self.preserve_pop(RDX, r);
        self.preserve_pop(RCX, r);
        self.preserve_pop(RBX, r);
        self.preserve_pop(RAX, r);
    }

    pub unsafe fn preserve_pop(&mut self, src: Register, reg: Register) {
        if src == reg {
            self.pop(SCRATCH);
        } else {
            self.pop(src);
        }
    }

    pub unsafe fn untag(&mut self, reg: Register) {
        emit_shr_reg_imm(&mut self.asm, 1, reg, 1);
    }
    pub unsafe fn tag_number(&mut self, reg: Register) {
        emit_shlq_reg(&mut self.asm, 1, reg);
    }

    pub unsafe fn allocate(
        &mut self,
        tag: HeapTag,
        size_reg: Register,
        size: usize,
        result: Register,
    ) {
        if result != RAX {
            self.push(RAX);
            self.push(RAX);
        }

        if size_reg == kNoRegister {
            (**self).mov(true, HNumber::tag(size as i64 + 8), RAX);
        } else {
            (**self).mov(true, size_reg, RAX);
            self.untag(RAX);
            emit_addq_imm_reg(&mut self.asm, 8 as _, RAX);
            self.tag_number(RAX);
        }
        self.push(RAX);
        (**self).mov(false, HNumber::tag(tag as i64) as i32, RAX);
        self.push(RAX);
        // TODO: Call allocation stub.
        if result != RAX {
            (**self).mov(true, RAX, result);
            self.pop(RAX);
            self.pop(RAX);
        }
    }
}

use std::ops::{Deref, DerefMut};

impl Deref for Masm {
    type Target = Assembler;
    fn deref(&self) -> &Assembler {
        &self.asm
    }
}

impl DerefMut for Masm {
    fn deref_mut(&mut self) -> &mut Assembler {
        &mut self.asm
    }
}
