#include <stdint.h>
#include <stdlib.h>

int32_t *c_alloc_four_i32(void) {
    int32_t *p = (int32_t *)malloc(4 * sizeof(int32_t));
    if (p != NULL) {
        p[0]=1; p[1]=2; p[2]=3; p[3]=4;
    }
    return p;
}
