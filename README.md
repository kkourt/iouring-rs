# ll_linuxio

A Rust library for using Linux io_uring.

# Notes

This implementation operates directly on the kernel ABI, while trying to follow
the [liburing](https://github.com/axboe/liburing) implementation. The goal is to
understand the `io_uring` facility and how to write low-level code with Rust.

Implementing bidings to `liburing` is probably a better choice in the long run,
while there seems to be at least one more Rust
[implementation](https://github.com/quininer/linux-io-uring) that operates
directly on the kernel ABI.
