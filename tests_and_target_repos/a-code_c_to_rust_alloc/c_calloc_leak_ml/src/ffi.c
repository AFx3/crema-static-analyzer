#include <stdint.h>
#include <stdlib.h>

uint64_t *c_calloc_u64(void) {
    return (uint64_t *)calloc(1, sizeof(uint64_t));
}
