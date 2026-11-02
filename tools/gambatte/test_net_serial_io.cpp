#include "net_serial_io.h"
#include <cassert>
#include <chrono>
#include <cerrno>
#include <sys/socket.h>
#include <sys/time.h>
#include <thread>
#include <unistd.h>

static void fragmented_input() {
    int fd[2];
    assert(socketpair(AF_UNIX, SOCK_STREAM, 0, fd) == 0);
    std::thread writer([&] {
        unsigned char first = 0x7B, second = 0x01;
        assert(send(fd[1], &first, 1, 0) == 1);
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        assert(send(fd[1], &second, 1, 0) == 1);
        close(fd[1]);
    });
    unsigned char pair[2] = {};
    assert(gambatteLinkRecvTwo(fd[0], pair));
    assert(pair[0] == 0x7B && pair[1] == 0x01);
    writer.join();
    assert(!gambatteLinkRecvTwo(fd[0], pair));
    close(fd[0]);
}

static void full_duplex() {
    int fd[2];
    assert(socketpair(AF_UNIX, SOCK_STREAM, 0, fd) == 0);
    std::thread peer([&] {
        for (unsigned i = 0; i < 128; i++) {
            unsigned char got[2] = {};
            assert(gambatteLinkRecvTwo(fd[1], got));
            assert(got[0] == static_cast<unsigned char>(i));
            assert(got[1] == 0x80);
            unsigned char answer[2] = {static_cast<unsigned char>(255 - i), 0x01};
            assert(gambatteLinkSendTwo(fd[1], answer));
        }
    });
    for (unsigned i = 0; i < 128; i++) {
        unsigned char sent[2] = {static_cast<unsigned char>(i), 0x80};
        assert(gambatteLinkSendTwo(fd[0], sent));
        unsigned char received[2] = {};
        assert(gambatteLinkRecvTwo(fd[0], received));
        assert(received[0] == static_cast<unsigned char>(255 - i));
        assert(received[1] == 0x01);
    }
    peer.join();
    close(fd[0]); close(fd[1]);
}

static void timeout_is_not_a_valid_packet() {
    int fd[2];
    assert(socketpair(AF_UNIX, SOCK_STREAM, 0, fd) == 0);
    timeval wait {0, 50000};
    assert(setsockopt(fd[0], SOL_SOCKET, SO_RCVTIMEO, &wait, sizeof(wait)) == 0);
    unsigned char packet[2] = {};
    assert(!gambatteLinkRecvTwo(fd[0], packet));
    assert(errno == EAGAIN || errno == EWOULDBLOCK);
    close(fd[0]); close(fd[1]);
}

int main() {
    fragmented_input();
    full_duplex();
    timeout_is_not_a_valid_packet();
    return 0;
}
