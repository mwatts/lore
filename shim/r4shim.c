// LD_PRELOAD write attributor for the R4 probe (not committed anywhere).
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <unistd.h>

static int enabled = 0;
static __thread int busy = 0;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

#define SLOTS 4096
struct slot { char key[200]; unsigned long long bytes, calls; };
static struct slot table[SLOTS];

static unsigned long long thread_wb(void) {
    char buf[1024];
    int fd = syscall(SYS_openat, AT_FDCWD, "/proc/thread-self/io", O_RDONLY);
    if (fd < 0) return 0;
    long n = syscall(SYS_read, fd, buf, sizeof(buf) - 1);
    syscall(SYS_close, fd);
    if (n <= 0) return 0;
    buf[n] = 0;
    char *p = strstr(buf, "\nwrite_bytes:");
    if (!p) return 0;
    return strtoull(p + 13, 0, 10);
}

static void norm(const char *in, char *out, size_t cap) {
    const char *a = strstr(in, "/archive/");
    if (a) in = a + 9;
    size_t o = 0;
    for (size_t i = 0; in[i] && o + 2 < cap; i++) {
        char c = in[i];
        if ((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f' && i > 0 && in[i-1] != '/' && 0)) {
            if (o == 0 || out[o-1] != '#') out[o++] = '#';
        } else out[o++] = c;
    }
    out[o] = 0;
}

static void fdpath(int fd, char *out, size_t cap) {
    char link[64], target[512];
    snprintf(link, sizeof link, "/proc/self/fd/%d", fd);
    long n = syscall(SYS_readlinkat, AT_FDCWD, link, target, sizeof(target) - 1);
    if (n < 0) { snprintf(out, cap, "fd?"); return; }
    target[n] = 0;
    norm(target, out, cap);
}

static void add(const char *call, const char *path, unsigned long long delta) {
    char key[200];
    snprintf(key, sizeof key, "%s %s", call, path);
    unsigned h = 5381;
    for (char *p = key; *p; p++) h = h * 33 + (unsigned char)*p;
    pthread_mutex_lock(&lock);
    for (unsigned i = 0; i < SLOTS; i++) {
        struct slot *s = &table[(h + i) % SLOTS];
        if (!s->key[0]) { strcpy(s->key, key); }
        if (!strcmp(s->key, key)) { s->bytes += delta; s->calls++; break; }
    }
    pthread_mutex_unlock(&lock);
}

#define BEGIN unsigned long long before = 0; int track = enabled && !busy; if (track) { busy = 1; before = thread_wb(); }
#define END_FD(name, fd) if (track) { unsigned long long d = thread_wb() - before; char p[180]; fdpath(fd, p, sizeof p); add(name, p, d); busy = 0; }
#define END_PATH(name, path) if (track) { unsigned long long d = thread_wb() - before; char p[180]; norm(path ? path : "?", p, sizeof p); add(name, p, d); busy = 0; }

#define REAL(ret, name, ...) static ret (*real_##name)(__VA_ARGS__); if (!real_##name) real_##name = dlsym(RTLD_NEXT, #name);

ssize_t write(int fd, const void *b, size_t n) { REAL(ssize_t, write, int, const void *, size_t); BEGIN; ssize_t r = real_write(fd, b, n); END_FD("write", fd); return r; }
ssize_t pwrite64(int fd, const void *b, size_t n, off_t o) { REAL(ssize_t, pwrite64, int, const void *, size_t, off_t); BEGIN; ssize_t r = real_pwrite64(fd, b, n, o); END_FD("pwrite", fd); return r; }
ssize_t pwrite(int fd, const void *b, size_t n, off_t o) { REAL(ssize_t, pwrite, int, const void *, size_t, off_t); BEGIN; ssize_t r = real_pwrite(fd, b, n, o); END_FD("pwrite", fd); return r; }
ssize_t pwritev(int fd, const struct iovec *v, int c, off_t o) { REAL(ssize_t, pwritev, int, const struct iovec *, int, off_t); BEGIN; ssize_t r = real_pwritev(fd, v, c, o); END_FD("pwritev", fd); return r; }
ssize_t pwritev64(int fd, const struct iovec *v, int c, off_t o) { REAL(ssize_t, pwritev64, int, const struct iovec *, int, off_t); BEGIN; ssize_t r = real_pwritev64(fd, v, c, o); END_FD("pwritev", fd); return r; }
ssize_t writev(int fd, const struct iovec *v, int c) { REAL(ssize_t, writev, int, const struct iovec *, int); BEGIN; ssize_t r = real_writev(fd, v, c); END_FD("writev", fd); return r; }
int fsync(int fd) { REAL(int, fsync, int); BEGIN; int r = real_fsync(fd); END_FD("fsync", fd); return r; }
int fdatasync(int fd) { REAL(int, fdatasync, int); BEGIN; int r = real_fdatasync(fd); END_FD("fdatasync", fd); return r; }
int ftruncate(int fd, off_t l) { REAL(int, ftruncate, int, off_t); BEGIN; int r = real_ftruncate(fd, l); END_FD("ftruncate", fd); return r; }
int ftruncate64(int fd, off_t l) { REAL(int, ftruncate64, int, off_t); BEGIN; int r = real_ftruncate64(fd, l); END_FD("ftruncate", fd); return r; }
int rename(const char *a, const char *b) { REAL(int, rename, const char *, const char *); BEGIN; int r = real_rename(a, b); END_PATH("rename", b); return r; }
int renameat(int d1, const char *a, int d2, const char *b) { REAL(int, renameat, int, const char *, int, const char *); BEGIN; int r = real_renameat(d1, a, d2, b); END_PATH("rename", b); return r; }
int renameat2(int d1, const char *a, int d2, const char *b, unsigned f) { REAL(int, renameat2, int, const char *, int, const char *, unsigned); BEGIN; int r = real_renameat2(d1, a, d2, b, f); END_PATH("rename", b); return r; }
int unlink(const char *a) { REAL(int, unlink, const char *); BEGIN; int r = real_unlink(a); END_PATH("unlink", a); return r; }
int unlinkat(int d, const char *a, int f) { REAL(int, unlinkat, int, const char *, int); BEGIN; int r = real_unlinkat(d, a, f); END_PATH("unlink", a); return r; }
int mkdir(const char *a, mode_t m) { REAL(int, mkdir, const char *, mode_t); BEGIN; int r = real_mkdir(a, m); END_PATH("mkdir", a); return r; }
int open64(const char *p, int fl, ...) { REAL(int, open64, const char *, int, ...); va_list ap; va_start(ap, fl); mode_t m = va_arg(ap, int); va_end(ap); BEGIN; int r = real_open64(p, fl, m); if (fl & O_CREAT) { END_PATH("open+creat", p); } else if (track) busy = 0; return r; }
int open(const char *p, int fl, ...) { REAL(int, open, const char *, int, ...); va_list ap; va_start(ap, fl); mode_t m = va_arg(ap, int); va_end(ap); BEGIN; int r = real_open(p, fl, m); if (fl & O_CREAT) { END_PATH("open+creat", p); } else if (track) busy = 0; return r; }
int openat(int d, const char *p, int fl, ...) { REAL(int, openat, int, const char *, int, ...); va_list ap; va_start(ap, fl); mode_t m = va_arg(ap, int); va_end(ap); BEGIN; int r = real_openat(d, p, fl, m); if (fl & O_CREAT) { END_PATH("open+creat", p); } else if (track) busy = 0; return r; }
int openat64(int d, const char *p, int fl, ...) { REAL(int, openat64, int, const char *, int, ...); va_list ap; va_start(ap, fl); mode_t m = va_arg(ap, int); va_end(ap); BEGIN; int r = real_openat64(d, p, fl, m); if (fl & O_CREAT) { END_PATH("open+creat", p); } else if (track) busy = 0; return r; }
int close(int fd) { REAL(int, close, int); BEGIN; char p[180] = ""; if (track) fdpath(fd, p, sizeof p); int r = real_close(fd); if (track) { add("close", p, thread_wb() - before); busy = 0; } return r; }

void r4shim_reset(int on) {
    pthread_mutex_lock(&lock);
    memset(table, 0, sizeof table);
    pthread_mutex_unlock(&lock);
    enabled = on;
}

static int cmp(const void *a, const void *b) {
    const struct slot *x = a, *y = b;
    return x->bytes < y->bytes ? 1 : x->bytes > y->bytes ? -1 : 0;
}

void r4shim_dump(const char *label) {
    pthread_mutex_lock(&lock);
    static struct slot copy[SLOTS];
    memcpy(copy, table, sizeof table);
    pthread_mutex_unlock(&lock);
    qsort(copy, SLOTS, sizeof(struct slot), cmp);
    unsigned long long total = 0;
    for (int i = 0; i < SLOTS; i++) total += copy[i].bytes;
    fprintf(stderr, "R4SHIM %s total %llu\n", label, total);
    for (int i = 0; i < 40 && copy[i].bytes; i++)
        fprintf(stderr, "R4SHIM %s %12llu B %8llu calls  %s\n", label, copy[i].bytes, copy[i].calls, copy[i].key);
}
