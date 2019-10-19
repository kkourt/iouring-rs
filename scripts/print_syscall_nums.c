/*
 * Kornilios Kourtis <kkourt@kkourt.io>
 *
 * vim: set expandtab softtabstop=4 tabstop=4 shiftwidth=4:
 */

#define _GNU_SOURCE

#include <stdio.h>
#include <sys/ioctl.h>
#include <linux/fs.h>

#define PR_SYSCALL_NR(x) do {                                              \
    printf("pub const SYS_%s: c_long = 0x%lx;\n", #x, (long)(__NR_ ##x));    \
} while (0)

#define PR_IOCTL_NR(x) do {                                                \
    printf("pub const IOC_%s: c_long = 0x%lx;\n", #x, (long)(x));    \
} while(0)

int main(int argc, char *argv[])
{
    PR_SYSCALL_NR(epoll_create);
    PR_SYSCALL_NR(io_uring_register);
    PR_SYSCALL_NR(io_uring_enter);
    PR_SYSCALL_NR(io_uring_setup);
    PR_IOCTL_NR(BLKGETSIZE64);
    return 0;
}
