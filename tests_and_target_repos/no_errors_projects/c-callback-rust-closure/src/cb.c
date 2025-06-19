#include <stdio.h>

static void (*cb)(void *arg) = NULL;
static void *arg = NULL;

void register_cb(void (*c)(void *), void *a) {
    cb = c;
    arg = a;
}

void trigger_cb() {
    printf("Before calling cb\n");
    if (cb)
        cb(arg);
    printf("After calling cb\n");
}
