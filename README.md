# ll_linuxio

A Rust library for using Linux IO facilities in Rust

The first thing I want to implement is io_uring, and then move to epoll,
userfaultfd, or others.


# Notes

For `io_uring`, my implementation tries to follow `liburing`, but operates
directly on the kernel ABI. Implementing bindings to `liburing` might have been
a more "productive" approach, but not as interesting for me.

# References

- There seems to be another `io_uring` implementation in Rust:
  https://github.com/quininer/linux-io-uring but I have not looked into it.
