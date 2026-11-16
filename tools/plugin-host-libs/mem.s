# The memory functions and unwinder stubs of a library the emulator's interface-7
# loader takes (see mem.rs), in Intel syntax. `paperpaw-dev build --device emulator`
# links the same into an author's plugin.
.text
.globl memcpy
.hidden memcpy
.type memcpy,@function
memcpy:
mov rax, rdi
mov rcx, rdx
rep movsb
ret
.globl memmove
.hidden memmove
.type memmove,@function
memmove:
mov rax, rdi
mov rcx, rdx
cmp rdi, rsi
jbe 2f
lea r8, [rsi + rdx]
cmp rdi, r8
jae 2f
std
lea rdi, [rdi + rdx - 1]
lea rsi, [rsi + rdx - 1]
rep movsb
cld
ret
2:
rep movsb
ret
.globl memset
.hidden memset
.type memset,@function
memset:
mov r8, rdi
mov eax, esi
mov rcx, rdx
rep stosb
mov rax, r8
ret
.globl bcmp
.hidden bcmp
.type bcmp,@function
bcmp:
mov rcx, rdx
xor eax, eax
test rcx, rcx
jz 3f
repe cmpsb
setne al
3:
ret
.globl memcmp
.hidden memcmp
.type memcmp,@function
memcmp:
mov rcx, rdx
xor eax, eax
test rcx, rcx
jz 4f
repe cmpsb
je 4f
movzx eax, byte ptr [rdi - 1]
movzx ecx, byte ptr [rsi - 1]
sub eax, ecx
4:
ret
# `core` and `alloc` come as the toolchain built them, with unwinding
# paths that name these; the library is `panic = "abort"` and never
# unwinds, so they are never reached.
.globl rust_eh_personality
.hidden rust_eh_personality
.type rust_eh_personality,@function
rust_eh_personality:
ud2
.globl _Unwind_Resume
.hidden _Unwind_Resume
.type _Unwind_Resume,@function
_Unwind_Resume:
ud2
