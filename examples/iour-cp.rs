/*
 * Kornilios Kourtis <kkourt@kkourt.io>
 *
 * vim: set expandtab softtabstop=4 tabstop=4 shiftwidth=4:
 */

// cp using io_uring, following liburing/examples/io_uring-cp.c

use libc;
use ll_linuxio::io_uring;

const QD : libc::c_uint = 64;
const BS : libc::c_uint = (32*1024);


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


    let iour = match io_uring::IoUring::init(4) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("Failed to initioalize io_uring: {}", e);
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
}
