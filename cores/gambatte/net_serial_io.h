#pragma once
// Gambatte Game Link: one serial transfer is always exactly TWO bytes.
// Android/Linux only. Caller configures SO_RCVTIMEO and SO_SNDTIMEO.
#ifndef _WIN32
#include <cerrno>
#include <cstddef>
#include <sys/socket.h>
#include <unistd.h>

static inline bool gambatteLinkSendTwo(int fd, const unsigned char* bytes) {
    size_t sent = 0;
    while (sent < 2) {
        const ssize_t n = ::send(fd, bytes + sent, 2 - sent, MSG_NOSIGNAL);
        if (n > 0) { sent += static_cast<size_t>(n); continue; }
        if (n < 0 && errno == EINTR) continue;
        if (n == 0) errno = EPIPE;
        return false;
    }
    return true;
}

static inline bool gambatteLinkRecvTwo(int fd, unsigned char* bytes) {
    size_t received = 0;
    while (received < 2) {
        const ssize_t n = ::recv(fd, bytes + received, 2 - received, 0);
        if (n > 0) { received += static_cast<size_t>(n); continue; }
        if (n < 0 && errno == EINTR) continue;
        if (n == 0) errno = ECONNRESET;
        return false;
    }
    return true;
}
#endif
