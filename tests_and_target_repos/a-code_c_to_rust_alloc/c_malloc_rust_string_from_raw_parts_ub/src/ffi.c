#include <stdint.h>
#include <stdlib.h>

uint8_t *c_alloc_hello(void) {
    uint8_t *p = (uint8_t *)malloc(6);
    if (p != NULL) {
        p[0] = 'h';
        p[1] = 'e';
        p[2] = 'l';
        p[3] = 'l';
        p[4] = 'o';
        p[5] = '\0';
    }
    return p;
}
