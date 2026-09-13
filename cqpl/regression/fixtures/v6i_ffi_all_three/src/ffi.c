#include <stddef.h>
#include <stdlib.h>

void *crema_v6i_alloc(size_t n) {
    return malloc(n);
}

void crema_v6i_free(void *p) {
    free(p);
}
