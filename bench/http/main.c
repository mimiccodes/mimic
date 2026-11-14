/* http: a hello-world server, single-threaded with kqueue (macOS/BSD) or
 * epoll (Linux), keep-alive connections; bench/run.py measures it with
 * load/main.go */
#define _GNU_SOURCE /* memmem on glibc */
#include <arpa/inet.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#ifdef __linux__
#include <sys/epoll.h>
#else
#include <sys/event.h>
#endif
#include <unistd.h>

#define BUF 8192
typedef struct { int fd; size_t n; char in[BUF]; } conn;

static const char RESP[] = "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 13\r\n\r\nHello, World!";

int main(void) {
    int l = socket(AF_INET, SOCK_STREAM, 0), on = 1;
    setsockopt(l, SOL_SOCKET, SO_REUSEADDR, &on, sizeof on);
    struct sockaddr_in a = {0};
    a.sin_family = AF_INET;
    a.sin_port = htons(18432);
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (bind(l, (struct sockaddr *)&a, sizeof a) || listen(l, 1024)) return 1;
    fcntl(l, F_SETFL, O_NONBLOCK);
#ifdef __linux__
    int ep = epoll_create1(0);
    struct epoll_event ev = {.events = EPOLLIN, .data.ptr = NULL};
    epoll_ctl(ep, EPOLL_CTL_ADD, l, &ev);
    struct epoll_event evs[256];
#define WATCH(fd, c) do { ev.events = EPOLLIN; ev.data.ptr = (c); epoll_ctl(ep, EPOLL_CTL_ADD, (fd), &ev); } while (0)
#define WAIT() epoll_wait(ep, evs, 256, -1)
#define DATA(e) ((e).data.ptr)
#else
    int kq = kqueue();
    struct kevent ev;
    EV_SET(&ev, l, EVFILT_READ, EV_ADD, 0, 0, NULL);
    kevent(kq, &ev, 1, NULL, 0, NULL);
    struct kevent evs[256];
#define WATCH(fd, c) do { EV_SET(&ev, (fd), EVFILT_READ, EV_ADD, 0, 0, (c)); kevent(kq, &ev, 1, NULL, 0, NULL); } while (0)
#define WAIT() kevent(kq, NULL, 0, evs, 256, NULL)
#define DATA(e) ((e).udata)
#endif
    for (;;) {
        int n = WAIT();
        for (int i = 0; i < n; i++) {
            if (DATA(evs[i]) == NULL) { /* the listening socket */
                int fd;
                while ((fd = accept(l, NULL, NULL)) >= 0) {
                    fcntl(fd, F_SETFL, O_NONBLOCK);
                    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &on, sizeof on);
                    conn *c = calloc(1, sizeof *c);
                    c->fd = fd;
                    WATCH(fd, c);
                }
                continue;
            }
            conn *c = DATA(evs[i]);
            ssize_t r = read(c->fd, c->in + c->n, BUF - c->n);
            if (r <= 0) {
                close(c->fd);
                free(c);
                continue;
            }
            c->n += (size_t)r;
            /* answer every complete request (headers end with a blank line),
             * all the answers in one write */
            char *p = c->in, *end = c->in + c->n, *h;
            static char out[(BUF / 4 + 1) * (sizeof RESP)]; /* a request is at least 4 bytes */
            size_t o = 0;
            while ((h = memmem(p, (size_t)(end - p), "\r\n\r\n", 4))) {
                memcpy(out + o, RESP, sizeof RESP - 1);
                o += sizeof RESP - 1;
                p = h + 4;
            }
            if (o) write(c->fd, out, o);
            c->n = (size_t)(end - p);
            memmove(c->in, p, c->n);
        }
    }
}
