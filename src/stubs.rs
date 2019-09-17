use crate::asm::*;
use assembler_x64::*;
use constants_x64::*;
use generic::*;
use jazz_jit::*;

impl Masm {
    pub unsafe fn prologue(&mut self) {
        self.push(RBP);
        (**self).mov(1, RSP, RBP);
    }

    pub unsafe fn epilogue(&mut self, args: u16) {
        (**self).mov(1, RBP, RSP);
        self.pop(RBP);
        emit_retq_imm(&mut self.asm, args * 8);
    }
}
