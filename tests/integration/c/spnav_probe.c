/* spnav_probe: minimal libspnav client used by the integration tests.
 * Connects (honours SPNAV_SOCKET), prints device info, then prints events
 * as JSON lines for N seconds. Build: cc spnav_probe.c -lspnav -o spnav_probe */
#include <spnav.h>
#include <stdio.h>
#include <stdlib.h>
#include <poll.h>
#include <time.h>

int main(int argc, char **argv) {
    int secs = argc > 1 ? atoi(argv[1]) : 10;
    if (spnav_open() == -1) { fprintf(stderr, "spnav_open failed\n"); return 1; }
    spnav_client_name("spnav_probe");
    char name[256] = "";
    int proto = spnav_protocol();
    spnav_dev_name(name, sizeof name);
    printf("{\"proto\":%d,\"dev_name\":\"%s\",\"naxes\":%d,\"nbuttons\":%d,\"devtype\":%d}\n",
           proto, name, spnav_dev_axes(), spnav_dev_buttons(), spnav_dev_type());
    fflush(stdout);
    struct pollfd p = { spnav_fd(), POLLIN, 0 };
    time_t end = time(NULL) + secs;
    spnav_event ev;
    while (time(NULL) < end) {
        if (poll(&p, 1, 200) <= 0) continue;
        while (spnav_poll_event(&ev)) {
            if (ev.type == SPNAV_EVENT_MOTION)
                printf("{\"m\":[%d,%d,%d,%d,%d,%d],\"p\":%u}\n", ev.motion.x, ev.motion.y, ev.motion.z,
                       ev.motion.rx, ev.motion.ry, ev.motion.rz, ev.motion.period);
            else if (ev.type == SPNAV_EVENT_BUTTON)
                printf("{\"b\":%d,\"press\":%d}\n", ev.button.bnum, ev.button.press);
            fflush(stdout);
        }
    }
    spnav_close();
    return 0;
}
