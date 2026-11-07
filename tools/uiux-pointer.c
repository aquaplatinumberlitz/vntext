/* Private KWin input fixture. Never connects to the user's desktop.
 * Protocol: https://invent.kde.org/libraries/plasma-wayland-protocols/-/blob/master/src/protocols/fake-input.xml
 * Build: cc tools/uiux-pointer.c -o target/uiux-pointer -lwayland-client -lm
 */
#include <wayland-client.h>
#include <linux/input-event-codes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Version 3 has primitive arguments only; no generated protocol files are needed. */
static const struct wl_message requests[] = {
    {"authenticate", "ss", NULL}, {"pointer_motion", "ff", NULL},
    {"button", "uu", NULL}, {"axis", "uf", NULL},
    {"touch_down", "2uff", NULL}, {"touch_motion", "2uff", NULL},
    {"touch_up", "2u", NULL}, {"touch_cancel", "2", NULL},
    {"touch_frame", "2", NULL}, {"pointer_motion_absolute", "3ff", NULL},
};
static const struct wl_interface interface = {"org_kde_kwin_fake_input", 3, 10, requests, 0, NULL};
static struct wl_proxy *input;

static void found(void *data, struct wl_registry *registry, uint32_t name, const char *type, uint32_t version)
{
    if (!strcmp(type, interface.name) && version >= 3)
        input = wl_registry_bind(registry, name, &interface, 3);
}
static void gone(void *data, struct wl_registry *registry, uint32_t name) {}
static const struct wl_registry_listener listener = {found, gone};

static void move(struct wl_display *display, double x, double y)
{
    wl_proxy_marshal_flags(input, 9, NULL, 3, 0, wl_fixed_from_double(x), wl_fixed_from_double(y));
    wl_display_roundtrip(display);
}
static void button(struct wl_display *display, uint32_t state)
{
    wl_proxy_marshal_flags(input, 2, NULL, 3, 0, BTN_LEFT, state);
    wl_display_roundtrip(display);
}

int main(int argc, char **argv)
{
    const char *socket = getenv("WAYLAND_DISPLAY");
    const char *runtime = getenv("XDG_RUNTIME_DIR");
    const char *test = getenv("VNTEXT_UI_TEST");
    if (argc != 5 || !socket || strcmp(socket, "vntext-uiux-test") ||
        !test || strcmp(test, "isolated") || !runtime ||
        strncmp(runtime, "/tmp/vntext-uiux-", strlen("/tmp/vntext-uiux-"))) return 90;
    struct wl_display *display = wl_display_connect("vntext-uiux-test");
    if (!display) return 91;
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &listener, NULL);
    wl_display_roundtrip(display);
    if (!input) { fprintf(stderr, "Private KWin input protocol unavailable\n"); return 92; }
    wl_proxy_marshal_flags(input, 0, NULL, 3, 0, "vntext isolated UI test", "Private compositor only");
    wl_display_roundtrip(display);
    double x = atof(argv[1]), y = atof(argv[2]);
    move(display, x, y);
    usleep(150000);
    button(display, 1);
    usleep(300000);
    for (int i = 1; i <= 5; ++i) {
        move(display, x + atof(argv[3]) * i / 5, y + atof(argv[4]) * i / 5);
        usleep(60000);
    }
    usleep(500000);
    button(display, 0);
    usleep(100000);
    wl_display_disconnect(display);
    return 0;
}
