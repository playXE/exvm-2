use crate::asm::*;
use crate::heap::*;
use assembler::*;
use assembler_x64::*;
use constants_x64::*;
use generic::*;
use jazz_jit::*;

unsafe fn entry_stub() -> *const u8 {
    let mut asm = Masm::new();
    asm.prologue();

    (*asm).mov(true, ROOT_REG, RDI);
    (*asm).push(RBP);
    (*asm).push(RBX);
    (*asm).push(R11);
    (*asm).push(R12);
    (*asm).push(R13);
    (*asm).push(R14);
    (*asm).push(R15);

    let even = (*asm).create_label();
    let args = (*asm).create_label();
    let args_loop = (*asm).create_label();
    let unwind_even = (*asm).create_label();

    (*asm).mov(true, RSI, SCRATCH);
    asm.untag(SCRATCH);

    (*asm).load_int_const(MachineMode::Int8, RAX, 1);
    emit_testl_reg_reg(&mut *asm, SCRATCH, RAX);
    (*asm).jump_if(CondCode::Equal, even);
    (*asm).load_int_const(MachineMode::Int8, RAX, 0);
    emit_pushq_reg(&mut *asm, RAX);
    (*asm).bind_label(even);
    (*asm).mov(true, SCRATCH, RBX);
    emit_shlq_reg(&mut *asm, 3, RBX);
    emit_add_reg_reg(&mut *asm, 1, RDX, RBX);
    (*asm).jump(args_loop);
    (*asm).bind_label(args);

    emit_subq_imm_reg(&mut *asm, 8, RBX);
    (*asm).load_mem(MachineMode::Int64, Reg::Gpr(RAX), Mem::Base(RBX, 0));
    emit_pushq_reg(&mut *asm, RAX);

    (*asm).bind_label(args_loop);
    (*asm).cmp_reg(MachineMode::Int64, RBX, RDX);
    (*asm).jump_if(CondCode::NotEqual, args);
    emit_xor_reg_reg(&mut *asm, 1, RAX, RAX);
    emit_xor_reg_reg(&mut *asm, 1, RBX, RBX);
    emit_xor_reg_reg(&mut *asm, 1, RCX, RCX);
    emit_xor_reg_reg(&mut *asm, 1, RDX, RDX);
    emit_xor_reg_reg(&mut *asm, 1, R8, R8);
    emit_xor_reg_reg(&mut *asm, 1, R9, R9);
    emit_xor_reg_reg(&mut *asm, 1, R10, R10);
    emit_xor_reg_reg(&mut *asm, 1, R11, R11);
    emit_xor_reg_reg(&mut *asm, 1, R12, R12);
    emit_xor_reg_reg(&mut *asm, 1, R13, R13);
    emit_xor_reg_reg(&mut *asm, 1, R14, R14);
    emit_xor_reg_reg(&mut *asm, 1, R15, R15);

    // TODO: Spill RSI?

    (*asm).mov(true, RSI, RAX);
    (*asm).mov(true, RDI, SCRATCH);
    emit_callq_reg(&mut *asm, SCRATCH);

    asm.untag(RSI);
    asm.push(R15);
    (*asm).load_int_const(MachineMode::Int8, R15, 1);
    emit_testl_reg_reg(&mut *asm, RSI, R15);
    asm.pop(R15);
    (*asm).jump_if(CondCode::Equal, unwind_even);
    emit_addq_imm_reg(&mut *asm, 1, RSI);
    (*asm).bind_label(unwind_even);
    emit_shlq_reg(&mut *asm, 3, RSI);
    emit_add_reg_reg(&mut *asm, 1, RSP, RSI);
    emit_xor_reg_reg(&mut *asm, 1, RSI, RSI);

    asm.pop(R15);
    asm.pop(R14);
    asm.pop(R13);
    asm.pop(R12);
    asm.pop(R11);
    asm.pop(RBX);
    asm.pop(RBP);
    asm.epilogue(0);
    let mem = get_executable_memory(&*asm);
    mem.ptr()
}
