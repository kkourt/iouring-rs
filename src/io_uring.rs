//
// Kornilios Kourtis <kkourt@kkourt.io>
//
// vim: set expandtab softtabstop=4 tabstop=4 shiftwidth=4:
//

// Reference:
// kernel.dk/io_uring.pdf
// git://git.kernel.dk/liburing
//
// TODO:
//  - do the cp example
//  - port all io_uring_prep functions from liburing.h
//  - a configuration to pass to init()
//

use libc;
use std::mem;
use std::io;
use std::convert::{TryFrom,TryInto};

// use std::os::unix::io::{RawFd};

use backtrace::Backtrace;

/**
 * io_uring ABI
 */

/*
 * Syscall numbers for io_uring
 */
#[allow(non_upper_case_globals)]
pub const SYS_io_uring_register: libc::c_long = 427;
#[allow(non_upper_case_globals)]
pub const SYS_io_uring_enter: libc::c_long = 426;
#[allow(non_upper_case_globals)]
pub const SYS_io_uring_setup: libc::c_long = 425;

/*
 * Magic offsets for the application to mmap the data it needs
 */
const IORING_OFF_SQ_RING: i64 = 0;
const IORING_OFF_CQ_RING: i64 = 0x08000000;
const IORING_OFF_SQES:    i64 = 0x10000000;


type KernelRwf = libc::c_int;

// NB: There seems to be an RFC for anonymous unions, which might make declaring all these unions
// more concise, but it does not to be implemented as of now:
// - https://github.com/rust-lang/rfcs/pull/2102
// - https://github.com/rust-lang/rust/issues/49804

#[repr(C)]
union io_uring_sqe_args {
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

const IORING_OP_NOP             : u8 = 0;
const IORING_OP_READV           : u8 = 1;
const IORING_OP_WRITEV          : u8 = 2;
const IORING_OP_FSYNC           : u8 = 3;
const IORING_OP_READ_FIXED      : u8 = 4;
const IORING_OP_WRITE_FIXED     : u8 = 5;
const IORING_OP_POLL_ADD        : u8 = 6;
const IORING_OP_POLL_REMOVE     : u8 = 7;
const IORING_OP_SYNC_FILE_RANGE : u8 = 8;
const IORING_OP_SENDMSG         : u8 = 9;
const IORING_OP_RECVMSG         : u8 = 10;
const IORING_OP_INVALID         : u8 = 250; // Not part of the ABI, used internally

bitflags::bitflags!{
    struct SqeFlags: u8 {
        const FIXED_FILE    = 1 << 0; // use fixed fileset
        const IO_DRAIN      = 1 << 1; // issue after inflight IO
        const IO_LINK       = 1 << 2; // links next sqe
    }
}

bitflags::bitflags!{
    struct SetupFlags: u32 {
        const IOPOLL = 1 << 0; // io_context is polled
        const SQPOLL = 1 << 1; // SQ poll thread
        const SQ_AFF = 1 << 2; // sq_thread_cpu is valid
        const CQSIZE = 1 << 3; // app defined CQ size
    }
}

bitflags::bitflags!{
    struct SQFlags: u32 {
        const NEED_WAKEUP = 1 << 0; // needs io_uring_enter wakeup
    }
}

bitflags::bitflags!{
    struct EnterFlags: libc::c_uint {
        const GETEVENTS = 1<<0;
        const SQ_WAKEUP = 1<<1;
    }
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
    args: io_uring_sqe_args,
    user_data: u64,
    idx: io_uring_sqe_idx,
}

#[repr(C)]
struct io_uring_cqe {
    user_data: u64,   /* sqe->data submission passed back */
    res: i32,         /* result code for this event */
    flags: u32,
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

/**
 * Library structures
 */

/// Submission queue
//
// Most of the fields here are pointers to the mapped ring structure.
struct SQ {
    khead: *mut u32,
    ktail: *mut u32,
    kring_mask: *mut u32,
    kring_entries: *mut u32,
    kflags: *mut u32,
    kdropped: *mut u32,
    array: *mut u32,

    sqes: *mut io_uring_sqe,
    sqe_head: u32,
    sqe_tail: u32,

    ring_sz: libc::size_t,
    ring_ptr: *mut libc::c_void,
}

/// Completion queue
struct CQ {
    khead: *mut u32,
    ktail: *mut u32,
    kring_mask: *mut u32,
    kring_entries: *mut u32,
    overflow: *mut u32,

    cqes: *mut io_uring_sqe,

    ring_sz: libc::size_t,
    ring_ptr: *mut libc::c_void,
}


/// io uring descriptor
pub struct IoUring {
    fd: libc::c_int,
    sq: SQ,
    cq: CQ,
    flags: SetupFlags,
}

pub struct SQEntry(*mut io_uring_sqe);


/**
 * Syscall wrappers
 */

/// io_uring_register syscall wrapper
unsafe fn io_uring_register(
    fd: libc::c_int,
    opcode: libc::c_uint,
    arg: *mut libc::c_void,
    nr_args: libc::c_uint)
-> libc::c_long {
    libc::syscall(SYS_io_uring_register, fd, opcode, arg, nr_args)
}

/// io_uring_setup syscall wrapper
unsafe fn io_uring_setup(entries: libc::c_uint, params: *mut io_uring_params)
-> libc::c_int {
    let ret = libc::syscall(SYS_io_uring_setup, entries, params);
    libc::c_int::try_from(ret).unwrap_or(-1)
}

/// io_uring_enter syscall wrapper
unsafe fn io_uring_enter(
    fd: libc::c_int,
    to_submit: libc::c_uint,
    min_complete: libc::c_uint,
    flags: libc::c_uint,
    sigset: *mut libc::sigset_t)
-> libc::c_long {
    // NB: From looking at the kernel and liburing code, the sigset size needs to match the kernel
    // sigset size, which AFICT is different from sizeof(sigset_t) in userspace.
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


/**
 * Misc helpers
 */

/// mmap helper, using the default protection and flags
unsafe fn mmap(len: libc::size_t, fd: libc::c_int, off: libc::off_t) -> *mut libc::c_void {
    let prot  = libc::PROT_READ | libc::PROT_WRITE;
    let flags = libc::MAP_SHARED | libc::MAP_POPULATE;
    let null = 0 as *mut libc::c_void;
    libc::mmap(null, len, prot, flags, fd, off)
}

/// munmap helper
///
/// Prints a message at stder if munmap() returns an error.
unsafe fn munmap(addr: *mut libc::c_void, len: libc::size_t) -> libc::c_int {
        let err = libc::munmap(addr, len);
        if err == 0 {
            return err;
        }
        let bt = Backtrace::new();
        let errno_ptr = libc::__errno_location();
        let old_errno = *errno_ptr;
        let error = io::Error::from_raw_os_error(old_errno as i32);
        // NB: not sure how to print a backtrace here. There does not seem to be a way using libstd
        // that does not involve panic!
        eprintln!("WARNING: munmap() failed: {}\nBacktrace:\n{:?}", error, bt);
        *errno_ptr = old_errno;
        err
}

/// close() helper
///
/// Prints a message at stderr if close() returns an error.
unsafe fn close(fd: libc::c_int) -> libc::c_int {
        let err = libc::close(fd);
        if err == 0 {
            return err;
        }

        // Not match we can do in case of an error here. Just print a backtrace.
        let bt = Backtrace::new();
        let errno_ptr = libc::__errno_location();
        let old_errno = *errno_ptr;
        let error = io::Error::from_raw_os_error(old_errno as i32);
        eprintln!("WARNING: close() failed: {}\nBacktrace:\n{:?}", error, bt);
        *errno_ptr = old_errno;
        err
}

/**
 * Main implementation
 */

impl SQEntry {
    fn reset(&mut self) {
        let ptr = self.0;
        unsafe { *ptr =  mem::zeroed() };
    }

    fn prep_rw(&mut self, op: u8, fd: libc::c_int, addr: *const libc::c_void, len: u32, off: u64) {
        let sqe: &mut io_uring_sqe = unsafe { &mut *self.0 };
        *sqe = io_uring_sqe {
            opcode: op,
            flags: 0,
            ioprio: 0,
            fd: fd,
            off: off,
            addr: addr as u64,
            args: io_uring_sqe_args { rw_flags: 0 },
            user_data: 0,
            len: len,
            idx: io_uring_sqe_idx { __pad2: [0; 3] },
        };
    }

    pub fn set_data(&mut self, data: u64) {
        let sqe: &mut io_uring_sqe = unsafe { &mut *self.0 };
        sqe.user_data = data
    }

    pub fn prep_readv(&mut self, fd: libc::c_int, iovecs: *const libc::iovec, nr_vecs: u32, off: u64) {
        let ptr = iovecs as *const libc::c_void;
        self.prep_rw(IORING_OP_READV, fd, ptr, nr_vecs, off)
    }

    pub fn prep_writev(&mut self, fd: libc::c_int, iovecs: *const libc::iovec, nr_vecs: u32, off: u64) {
        let ptr = iovecs as *const libc::c_void;
        self.prep_rw(IORING_OP_READV, fd, ptr, nr_vecs, off)
    }

    /// This uses IoSlice, which is the buffer type ised in Write::write_vectored, and "is
    /// guaranteed to be ABI compatible with the iovec type on Unix platforms"
    //
    // NB: https://github.com/rust-lang/rust/blob/7bf377f289a4f79829309ed69dccfe33f20b089c/src/libstd/sys/unix/fd.rs#L103
    pub fn prep_write_slice(&mut self, fd: libc::c_int, bufs: &[std::io::IoSlice], off: u64) {
        self.prep_writev(
            fd,
            bufs.as_ptr() as *const libc::iovec,
            // NB: len() is usize, arg is u32. This will panic if a conversion cannot be made.
            bufs.len().try_into().unwrap(),
            off);
    }

    /// This uses IoSliceMut, which is the buffer type ised in Write::read_vectored, and "is
    /// guaranteed to be ABI compatible with the iovec type on Unix platforms"
    //
    // NB: https://github.com/rust-lang/rust/blob/7bf377f289a4f79829309ed69dccfe33f20b089c/src/libstd/sys/unix/fd.rs#L56
    pub fn prep_read_slice(&mut self, fd: libc::c_int, bufs: &[std::io::IoSliceMut], off: u64) {
        self.prep_readv(
            fd,
            bufs.as_ptr() as *const libc::iovec,
            // NB: len() is usize, arg is u32. This will panic if a conversion cannot be made.
            bufs.len().try_into().unwrap(),
            off);
    }

}

/// setup functions
impl IoUring {

    /// initialize an io uring
    pub fn init(nentries: libc::c_uint) -> io::Result<IoUring> {
        let mut params: io_uring_params = unsafe { std::mem::zeroed() };
        let params_p = &mut params as *mut io_uring_params;
        let fd = unsafe { io_uring_setup(nentries, params_p) };
        if fd < 0 {
            return Err(io::Error::last_os_error())
        }

        let mut ret : IoUring = IoUring {
            fd: fd,
            sq: unsafe { std::mem::zeroed() },
            cq: unsafe { std::mem::zeroed() },
            // NB: SetupFlags should be given by the user as an argument
            flags: SetupFlags::from_bits(params.flags).unwrap(),
        };

        let err = ret.queue_mmap(&mut params);
        if err.is_err() {
            unsafe { close(ret.fd); }
        }
        Ok(ret)
    }

    fn queue_mmap(&mut self, p: &mut io_uring_params) -> io::Result<()> {

        // convinience function for computing pointer offsets
        let ptr_off = |p: *const libc::c_void, off: u32| -> *mut libc::c_uint {
            let mut ptr = p as libc::uintptr_t;
            ptr += libc::uintptr_t::try_from(off).unwrap();
            ptr as *mut libc::c_uint
        };

        /*
         * mmap submission queue
         */
        let sq = &mut self.sq;

        // From io_uring_setup(2):
        // The addition of sq_off.array to the length of the region accounts for the fact that the
        // ring located at the end of the data structure.
        let sq_ring_sz  = {
            let s1 = libc::size_t::try_from(p.sq_off.array).unwrap();
            let s2 = libc::size_t::try_from(p.sq_entries).unwrap() * mem::size_of::<u32>();
            s1 + s2
        };

        // mmap the submission queue structure
        let sq_ring_ptr = {
            let ptr = unsafe { mmap(sq_ring_sz, self.fd, IORING_OFF_SQ_RING) };
            if ptr == libc::MAP_FAILED {
                return Err(io::Error::last_os_error())
            }
            ptr
        };

        let sqes_size = {
            let nentries = libc::size_t::try_from(p.sq_entries).unwrap();
            let esz = libc::size_t::try_from(mem::size_of::<io_uring_sqe>()).unwrap();
            nentries*esz
        };

        // mmap the submission queue entries array
        let sqes_ptr = {
            let sqp = unsafe { mmap(sqes_size, self.fd, IORING_OFF_SQES) };
            if sqp == libc::MAP_FAILED {
                unsafe { munmap(sq_ring_ptr, sq_ring_sz) };
                return Err(io::Error::last_os_error());
            }
            sqp as *mut io_uring_sqe
        };

        // initialize the SQ structure
        // setup pointers to submission queue structure using the sq offsets
        *sq = {
            let ptr = sq_ring_ptr;
            let off : &io_sqring_offsets = &p.sq_off;
            SQ {
                khead         : ptr_off(ptr, off.head),
                ktail         : ptr_off(ptr, off.tail),
                kring_mask    : ptr_off(ptr, off.ring_mask),
                kring_entries : ptr_off(ptr, off.ring_entries),
                kflags        : ptr_off(ptr, off.flags),
                kdropped      : ptr_off(ptr, off.dropped),
                array         : ptr_off(ptr, off.array),
                sqes          : sqes_ptr,
                sqe_head      : 0,
                sqe_tail      : 0,
                ring_sz       : sq_ring_sz,
                ring_ptr      : ptr,
            }
        };

        // these two have to be the same so that the unmap when closing io_uring works properly
        assert_eq!(p.sq_entries, unsafe { *sq.kring_entries });

        /*
         * mmap completion queue
         */
        let cq = &mut self.cq;

        let cq_ring_sz = {
            let s1 = libc::size_t::try_from(p.cq_off.cqes).unwrap();
            let s2 = libc::size_t::try_from(p.cq_entries).unwrap() * mem::size_of::<io_uring_cqe>();
            s1 + s2
        };

        let cq_ring_ptr  = {
            let ptr = unsafe { mmap(cq_ring_sz, self.fd, IORING_OFF_CQ_RING) };
            if ptr == libc::MAP_FAILED {
                unsafe {
                    munmap(sq_ring_ptr, sq_ring_sz);
                    munmap(sqes_ptr as *mut libc::c_void, sqes_size);
                }
                return Err(io::Error::last_os_error())
            }
            ptr
        };

        *cq = {
            let ptr = cq_ring_ptr;
            let off : &io_cqring_offsets = &p.cq_off;
            CQ {
                khead: ptr_off(ptr, off.head),
                ktail: ptr_off(ptr, off.tail),
                kring_mask: ptr_off(ptr, off.ring_mask),
                kring_entries: ptr_off(ptr, off.ring_entries),
                overflow: ptr_off(ptr, off.overflow),
                cqes: ptr_off(ptr, off.cqes) as *mut io_uring_sqe,
                ring_sz: cq_ring_sz,
                ring_ptr: ptr
            }
        };

        Ok(())
    }

    fn queue_unmap(&mut self) {
        let sqes_size = {
            let nentries_ = unsafe { *self.sq.kring_entries };
            let nentries = libc::size_t::try_from(nentries_).unwrap();
            let esz = libc::size_t::try_from(mem::size_of::<io_uring_sqe>()).unwrap();
            nentries*esz
        };
        unsafe {
            munmap(self.sq.ring_ptr, self.sq.ring_sz);
            munmap(self.sq.sqes as *mut libc::c_void, sqes_size);
            munmap(self.cq.ring_ptr, self.cq.ring_sz);
        }
    }


}

impl Drop for IoUring {
    fn drop(&mut self) {
        self.queue_unmap();
        unsafe { close(self.fd) };
    }
}


// queue functions: SQ
impl IoUring {

    /// Get a new submission queue entry (sqe)
    ///
    /// If queue is full, return None
    pub fn get_sqe(&mut self) -> Option<SQEntry> {
        let sq = &mut self.sq;
        let next: u32 = sq.sqe_tail + 1;
        let nentries: u32 = unsafe { *sq.kring_entries };
        if next - sq.sqe_head > nentries {
            return None
        }

        let mask = unsafe { *sq.kring_mask };
        let idx = sq.sqe_tail & mask;
        let sqe_p = unsafe { sq.sqes.offset(idx as isize) };

        sq.sqe_tail = next;
        Some(SQEntry(sqe_p))
    }

    /// Returns: sqes submited
    // liburing: __io_uring_flush_sq()
    fn flush_sq(&mut self) -> u32 {
        let sq = &mut self.sq;

        // NB: This works even if there is an overflow on sqe_{tail,head}
        let to_submit = sq.sqe_tail - sq.sqe_head;
        if to_submit == 0 {
            return 0
        }

        let mask = unsafe { *sq.kring_mask };
        let mut ktail = unsafe { *sq.ktail };
        let mut submitted = 0;
        loop  {
            // I don't see how this can overflow isize, so skip the runtime test
            let aoff = (ktail & mask) as isize;
            unsafe {
                *sq.array.offset(aoff) = sq.sqe_head & mask;
            }
            sq.sqe_head += 1;
            ktail += 1;
            submitted += 1;

            if submitted == to_submit {
                break;
            }
        }

        // Ensure that the queue consumer (kernel) to see the updated sqe entries before any
        // updates to the tail.
        //
        // NB: not sure if there is a better way to do this than the cast here, but AtomicU32
        // documentation says that: "This type has the same in-memory representation as the
        // underlying integer type, u32."
        let ktail_p = sq.ktail as *mut std::sync::atomic::AtomicU32;
        unsafe {
            (&*ktail_p).store(ktail, std::sync::atomic::Ordering::Release);
        }

        submitted
    }

    // Returns:
    // None -> No need to enter for the SQ (this will happen when SQPOLL is defined)
    // Some(flags) -> you need to enter for the SQ, please use the following flags
    //
    fn sq_ring_needs_enter(&mut self) -> Option<EnterFlags> {

        if !self.flags.contains(SetupFlags::SQPOLL) {
            return Some(EnterFlags::empty())
        }

        let need_wakeup = unsafe {
            let flags = std::ptr::read_volatile(self.sq.kflags);
            SQFlags::from_bits_unchecked(flags).contains(SQFlags::NEED_WAKEUP)
        };
        if need_wakeup {
            return Some(EnterFlags::SQ_WAKEUP);
        }

        None
    }

    // liburing: __io_uring_submit()
    fn do_submit(&mut self, submitted: u32, mut wait_nr: u32) -> std::io::Result<u32> {

        let flags = match (wait_nr, self.sq_ring_needs_enter()) {
            (0, None) => {
                // No need to issue system call, just return
                return Ok(submitted);
            },
            (0, Some(x)) => x,
            (_, None) => EnterFlags::GETEVENTS,
            (_, Some(mut x)) => {
                x.set(EnterFlags::GETEVENTS, true);
                x
            }
        };

        // NB: I guess liburing truncates wait_nr to submitted to avoid the case of sleeping
        // forever, even though waiting for more than you submit might be valid if you previously
        // submitted without waiting.
        if wait_nr > submitted {
            wait_nr = submitted;
        }

        let null = 0 as *mut libc::sigset_t;
        let ret = unsafe {
            io_uring_enter(self.fd, submitted, wait_nr, flags.bits(), null)
        };

        if ret < 0 {
            // wrap errno
            Err(std::io::Error::last_os_error())
        } else {
            Ok(ret as u32)
        }
    }

    // liburing: __io_uring_submit_and_wait
    fn do_submit_and_wait(&mut self, wait_nr: u32) -> std::io::Result<u32> {
        let submitted = self.flush_sq();
        if submitted > 0 {
            return self.do_submit(submitted, wait_nr)
        }
        Ok(0)
    }

    /// Submit sqes acquired via get_sqe() to the kernel.
    ///
    /// Returns number of sqes submitted, or error if io_uring_enter() failed.
    pub fn submit(&mut self) -> std::io::Result<u32> {
        self.do_submit_and_wait(0)
    }
}

// queue functions: CQ
impl IoUring {
}

impl IoUring {
    // /// Fill the next SQEntry in the queue via the provided function.
    // ///
    // /// Returns:
    // ///  None: queue is full (fill function was not executed)
    // ///  Some(Err(x)): fill function returned Err(x), queue was not updated
    // ///  Some(Err(x)): fill function returned Ok(x), queue was updated
    // pub fn fill_next_sqe<F, E, R>(&mut self, fill: F) -> Option<Result<E,R>>
    // where F: FnOnce(&mut SQEntry) -> Result<E,R> {
    //     let sq = &mut self.sq;
    //     let next: u32 = sq.sqe_tail + 1;
    //     let nentries: u32 = unsafe { *sq.kring_entries };
    //     if next - sq.sqe_head > nentries {
    //         return None
    //     }

    //     let mut sqe = {
    //         let mask = unsafe { *sq.kring_mask };
    //         let idx = sq.sqe_tail & mask;
    //         let sqe_p = unsafe { sq.sqes.offset(idx as isize) };
    //         SQEntry(sqe_p)
    //     };
    //     sqe.reset();

    //     let fret = fill(&mut sqe);
    //     if fret.is_ok() {
    //         // update tail to commit new entry
    //         sq.sqe_tail = next;
    //     }

    //     Some(fret)
    // }
}
