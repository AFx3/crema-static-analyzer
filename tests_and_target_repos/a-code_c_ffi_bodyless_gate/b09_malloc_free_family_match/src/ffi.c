#include <stddef.h>
#include <stdlib.h>

void *gate_seed_malloc(size_t n) {
    return malloc(n);
}

void gate_seed_free(void *p) {
    free(p);
}
