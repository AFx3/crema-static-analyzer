#include <stdint.h>
#include <stdlib.h>

int32_t *c_alloc_i32(int32_t value) {
    int32_t *p = (int32_t *)malloc(sizeof(int32_t));
    if (p != NULL) {
        *p = value;
    }
    return p;
}

void c_free_i32(int32_t *p) {
    free(p);
}
