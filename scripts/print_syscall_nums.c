/*
 * Kornilios Kourtis <kkourt@kkourt.io>
 *
 * vim: set expandtab softtabstop=4 tabstop=4 shiftwidth=4:
 */

#include <stdio.h>

#define PR_SYSCALL_NR(x) do {                                              \
    printf("pub const SYS_%s: ::c_long = %ld;\n", #x, (long)(__NR_ ##x));  \
} while (0)                                                                \

int main(int argc, char *argv[])
{
    PR_SYSCALL_NR(epoll_create);
    PR_SYSCALL_NR(io_uring_register);
    PR_SYSCALL_NR(io_uring_enter);
    PR_SYSCALL_NR(io_uring_setup);
    return 0;
}
