//
// Kornilios Kourtis <kkourt@kkourt.io>
//
// vim: set expandtab softtabstop=4 tabstop=4 shiftwidth=4:
//

use libc;
use std::mem;
use std::io;
use std::convert::TryFrom;

use crate::kernel_abi::{
    SYS_io_uring_register,
    SYS_io_uring_enter,
    SYS_io_uring_setup
};

/*
 * Magic offsets for the application to mmap the data it needs
 */
const IORING_OFF_SQ_RING: i64 = 0;
const IORING_OFF_CQ_RING: i64 = 0x8000000;
const IORING_OFF_SQES:    i64 = 0x10000000;

/// mmap helper
fn mmap(len: libc::size_t, fd: libc::c_int, off: libc::off_t) -> *mut libc::c_void {
    let prot  = libc::PROT_READ | libc::PROT_WRITE;
    let flags = libc::MAP_SHARED | libc::MAP_POPULATE;
    let null = 0 as *mut libc::c_void;
    unsafe {
        libc::mmap(null, len, prot, flags, fd, off)
    }
}

#[repr(C)]
struct io_uring_sq {
    khead: *mut libc::c_uint,
    ktail: *mut libc::c_uint,
    kring_masks: *mut libc::c_uint,
    kring_entries: *mut libc::c_uint,
    kflags: *mut libc::c_uint,
    kdropped: *mut libc::c_uint,
    array: *mut libc::c_uint,

    sqes: *mut io_uring_sqe,
    sqe_head: libc::c_uint,
    sqe_tail: libc::c_uint,

    ring_sz: libc::size_t,
    ring_ptr: *mut libc::c_void,
}

impl io_uring_sq {
    fn empty() -> io_uring_sq {
        unsafe { std::mem::zeroed() }
    }

    fn sques_size(&self) -> libc::size_t {
        let nentries_ = unsafe { *self.kring_entries };
        let nentries = libc::size_t::try_from(nentries_).unwrap();
        let esz = libc::size_t::try_from(mem::size_of::<io_uring_sqe>()).unwrap();
        nentries*esz
    }
}

pub struct IoUring {
    fd: libc::c_int,
    sq: io_uring_sq,
}

type KernelRwf = libc::c_int;

#[repr(C)]
union io_uring_sqe_arg {
    rw_flags: KernelRwf,
    fsync_flags: u32,
    poll_events: u16,
    sync_range_flags: u32,
}

#[repr(C)]
union io_uring_sqe_idx {
    buf_index: u16,
    __pad2: [u64; 3],
}

#[repr(C)]
struct io_uring_sqe {
    opcode: u8,                /* type of operation for this sqe */
    flags: u8,                 /* IOSQE_ flags */
    ioprio: u16,               /* ioprio for the request */
    fd: i32,                   /* file descriptor to do IO on */
    off: u64,                  /* offset into file */
    addr: u64,                 /* pointer to buffer or iovecs */
    len: u32,                  /* buffer size or number of iovecs */
    arg: io_uring_sqe_arg,
    user_data: u64,
    idx: io_uring_sqe_idx,
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

impl io_uring_params {
    fn get_sq_ring_size(&self) -> libc::size_t {
        let s1 = self.sq_off.array as libc::size_t;
        let s2 = (self.sq_entries as libc::size_t) * mem::size_of::<libc::c_uint>();
        s1 + s2
    }
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
-> libc::c_int {
    let ret = libc::syscall(SYS_io_uring_setup, entries, params);
    libc::c_int::try_from(ret).unwrap_or(-1)
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

        let mut ret : IoUring = IoUring {
            fd: fd,
            sq: io_uring_sq::empty()
        };

        let err = ret.queue_mmap(&mut params);
        if err.is_err() {
            unsafe { libc::close(ret.fd); }
        }
        Ok(ret)
    }



    fn queue_mmap(&mut self, p: &mut io_uring_params) -> io::Result<()> {

        let ptr_off = |p: *const libc::c_void, off: u32| -> *mut libc::c_uint {
            let mut ptr = p as libc::uintptr_t;
            ptr += libc::uintptr_t::try_from(off).unwrap();
            ptr as *mut libc::c_uint
        };

        let mut ring : IoUring = unsafe { std::mem::zeroed() };

        let sq = &mut ring.sq;
        sq.ring_sz  = p.get_sq_ring_size();
        sq.ring_ptr = mmap(sq.ring_sz, self.fd, IORING_OFF_SQ_RING);
        if sq.ring_ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error())
        }
        sq.khead         = ptr_off(sq.ring_ptr, p.sq_off.head);
        sq.ktail         = ptr_off(sq.ring_ptr, p.sq_off.tail);
        sq.kring_masks   = ptr_off(sq.ring_ptr, p.sq_off.ring_mask);
        sq.kring_entries = ptr_off(sq.ring_ptr, p.sq_off.ring_entries);
        sq.kflags        = ptr_off(sq.ring_ptr, p.sq_off.flags);
        sq.kdropped      = ptr_off(sq.ring_ptr, p.sq_off.dropped);
        sq.array         = ptr_off(sq.ring_ptr, p.sq_off.array);
        sq.sqes          = {
            let sqp = mmap(sq.sques_size(), self.fd, IORING_OFF_SQES);
            if sqp == libc::MAP_FAILED {
                unsafe { libc::munmap(sq.ring_ptr, sq.ring_sz) };
                return Err(io::Error::last_os_error());
            }
            sqp as *mut io_uring_sqe
        };

        unimplemented!()
    }
}
