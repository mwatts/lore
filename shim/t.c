#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <unistd.h>
int main(void) {
    void (*r)(int) = dlsym(RTLD_DEFAULT, "r4shim_reset");
    void (*d)(const char *) = dlsym(RTLD_DEFAULT, "r4shim_dump");
    r(1);
    int fd = open("/home/mwatts/code/github/mwatts/r4-scratch/shim/x.bin", O_CREAT | O_WRONLY | O_TRUNC, 0644);
    static char b[65536] = {1};
    for (int i = 0; i < 10; i++) write(fd, b, sizeof b);
    fsync(fd);
    close(fd);
    d("t");
    return 0;
}
