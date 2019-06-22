#![allow(dead_code)]

pub mod io_uring;
mod kernel_abi;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn hello() {
        let res = crate::io_uring::IoUring::init(4);
    }
}
