#include <stdlib.h>

void free_second(void *first, void *second) {
    (void)first;
    free(second);
}
