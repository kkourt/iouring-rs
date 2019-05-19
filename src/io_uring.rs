//
// Kornilios Kourtis <kkourt@kkourt.io>
//
// vim: set expandtab softtabstop=4 tabstop=4 shiftwidth=4:
//

use libc;
use std::io;

use crate::kernel_abi::{
    SYS_io_uring_register,
    SYS_io_uring_enter,
    SYS_io_uring_setup
};

pub struct IoUring {
    fd: libc::c_int,
}


#[repr(C)]
struct io_sqring_offsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    flags: u32,
    dropped: u32,
    array: u32,
    resv1: u32,
    resv2: u64,
}

#[repr(C)]
struct io_cqring_offsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    overflow: u32,
    cqes: u32,
    resv: [u64; 2],
}


#[repr(C)]
struct io_uring_params {
    sq_entries: u32,
    cq_entries: u32,
    flags: u32,
    sq_thread_cpu: u32,
    sq_thread_idle: u32,
    resv: [u32; 5],
    sq_off: io_sqring_offsets,
    cq_off: io_cqring_offsets,
}

unsafe fn io_uring_register(
    fd: libc::c_int,
    opcode: libc::c_uint,
    arg: *mut libc::c_void,
    nr_args: libc::c_uint)
-> libc::c_long {
    libc::syscall(SYS_io_uring_register, fd, opcode, arg, nr_args)
}

unsafe fn io_uring_setup(entries: libc::c_uint, params: *mut io_uring_params)
-> libc::c_long {
    libc::syscall(SYS_io_uring_setup, entries, params)
}

unsafe fn io_uring_enter(
    fd: libc::c_int,
    to_submit: libc::c_uint,
    min_complete: libc::c_uint,
    flags: libc::c_uint,
    sigset: *mut libc::sigset_t)
-> libc::c_long {
    // NB: From looking at the kernel code, the sigset size needs to match the kernel sigset size,
    // which I guess is different from sizeof(sigset_t) in userspace.
    //
    // References:
    //  liburing io_uring_enter wrapper
    //    http://git.kernel.dk/cgit/liburing/tree/src/syscall.c?id=1a90a51ecd678d4331990d7f696153b59583d378#n39
    //
    //  function called by io_uring_enter() syscall:
    //    https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/kernel/signal.c?h=v5.1#n2810
    //
    //  sigset kernel definition
    //    https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/x86/include/asm/signal.h?h=v5.1#n11
    //
    //  sigset GNU libc definition:
    //    http://www.sourceware.org/git/?p=glibc.git;a=blob;f=sysdeps/unix/sysv/linux/bits/types/__sigset_t.h;h=e2f18acf30f43496567b1511456089dcd1798425;hb=fef7c63cd5a5a3150dc9465687359351afab5010
    //    indeed, sizeof(sigset_t) is 128)
    //
    const NSIG_: libc::c_uint = 65;
    let sigset_size: libc::c_uint = NSIG_ / 8;
    libc::syscall(SYS_io_uring_enter, fd, to_submit, min_complete, flags, sigset, sigset_size)
}

impl IoUring {
    pub fn init(nentries: libc::c_uint) -> io::Result<IoUring> {
        let mut params: io_uring_params = unsafe { std::mem::zeroed() };
        let params_p = &mut params as *mut io_uring_params;
        let fd = unsafe { io_uring_setup(nentries, params_p) };
        if fd < 0 {
            return Err(io::Error::last_os_error())
        }
        unimplemented!()
    }
}
