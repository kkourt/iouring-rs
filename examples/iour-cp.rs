/*
 * Kornilios Kourtis <kkourt@kkourt.io>
 *
 * vim: set expandtab softtabstop=4 tabstop=4 shiftwidth=4:
 */

// cp using io_uring, following liburing/examples/io_uring-cp.c

use libc;
use iouring::io_uring;

// use std::ops::Deref;
use std::convert::TryInto;

const QD : usize = 64;
const BS : usize = (32*1024);

// rust uses IoSlice for write_vectored and IoSliceMut for read_vectored.
// _Both_ are guaranteed to be ABI compatible with iovec.
//
// From: rust.git/src/libstd/sys/unix/io.rs
// | ...
// | pub struct IoSlice<'a> {
// |    vec: iovec,
// |    _p: PhantomData<&'a [u8]>,
// | }
// | ...
// | pub struct IoSliceMut<'a> {
// |     vec: iovec,
// |     _p: PhantomData<&'a mut [u8]>,
// | }

/// Buffer for performing IO
struct IoBuff {
    off: usize,
    size: usize,
    buff: Vec<u8>,
    iov: libc::iovec,
}

impl IoBuff {

    pub fn new(size: usize, off: usize) -> IoBuff {
        IoBuff {
            off: off,
            size: size,
            buff: Vec::with_capacity(size),
            iov: libc::iovec {
                iov_base: buff.as_mut_ptr() as *mut libc::c_void,
                iov_len: size,
            },
        }
    }
}

/// get the size of the file, properly handling block devices
///
/// (fs::metdata -> len(), does not work for block devices)
fn get_file_size(f: &std::fs::File) -> std::io::Result<usize>  {

    pub const IOC_BLKGETSIZE64: libc::c_ulong = 0x80081272;

    let s_isreg = |m: u32| -> bool {
        (m & libc::S_IFMT) == libc::S_IFREG
    };

    let s_isblk = |m: u32| -> bool {
        (m & libc::S_IFMT) == libc::S_IFBLK
    };

    use std::os::unix::io::AsRawFd;
    let fd = f.as_raw_fd();

    let st: libc::stat  = unsafe {
        let mut ret: libc::stat = std::mem::zeroed();
        let err = libc::fstat(fd, &mut ret);
        if err != 0 {
            return Err(std::io::Error::from_raw_os_error(err));
        }
        ret
    };

    if s_isreg(st.st_mode) {
       return Ok(st.st_size as usize)
    } else if s_isblk(st.st_mode) {
        let mut bytes: libc::c_ulonglong = 0;
        let err = unsafe { libc::ioctl(fd, IOC_BLKGETSIZE64, &mut bytes) };
        if err == 0 {
            Ok(bytes as usize)
        } else {
            Err(std::io::Error::last_os_error())
        }
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "Cannot determine file size"))
    }
}

fn queue_read(ior: &mut io_uring::IoUring, fd: RawFd, size: usize, off: usize) -> Option<()> {

    // allocate entry in the submission queue
    let mut sqe = match ior.get_sqe() {
        Some(x) => x,
        None => return None,
    };

    let iodata = Box::new(IoBuff::new(size, off));
    sqe.prep_readv(fd, &iodata.iov, 1, off.try_into().unwrap());
    let iodata_ptr = Box::into_raw(iodata) as u64;
    sqe.set_data(iodata_ptr);
    Some(())
}

fn copy_file(ior: &io_uring::IoUring, infd: RawFd, insize: usize, outfd: RawFd) -> std::io::Result<()> {
    let mut rd_issued: usize = 0;
    let mut rd_done: usize = 0;
    let mut wr_issued: usize = 0;
    let mut wr_done: usize = 0;

    while wr_done < insize {

        // queue as many read requests as possible
        let mut rd_queued = 0;
        while rd_issued < insize {
            let rd_size = std::cmp::min(insize - rd_issued, BS);
            let rd_off = rd_issued;
            match queue_read(ior, infd, rd_size, rd_off) {
                None => break,
                Some(()) => {
                    rd_issued += rd_size;
                    rd_queued++;
                },
            }
        }

        // submit the read requests enqueued (if any)
        if rd_queued > 0 {
            ior.submit()?
        }
    }

}

pub fn main() {
    let mut args = std::env::args();

    let arg0 = &args.next().unwrap();
    if args.len() < 2 {
        // NB: This seems to be the equivalent of basename(argv[0]) in rust
        let pname = std::path::Path::new(arg0).file_name().unwrap().to_str().unwrap_or("iour-cp");
        eprintln!("Usage: {} <infile> <outfile>", pname);
        std::process::exit(-1);
    }

    let fin = {
        let arg1 = &args.next().unwrap();
        match std::fs::File::open(arg1) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Failed to open {}: {}", arg1, e);
                std::process::exit(-1);
            }
        }
    };

    let fout = {
        let arg2 = &args.next().unwrap();
        match std::fs::File::create(arg2) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Failed to create {}: {}", arg2, e);
                std::process::exit(-1);
            }
        }
    };


    let iour = match io_uring::IoUring::init(QD) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to initialize io_uring: {}", e);
            std::process::exit(-1);
        }
    };

    let insize = match get_file_size(&fin) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to get size of input file: {}", e);
            std::process::exit(-1);
        }
    };

    println!("insize={}", insize);
    println!("iodata::capacity={}", IoData::MAX_SIZE);
}
