#include <stdlib.h>

void free_second(int *first, int *second) {
    (void)first;
    free(second);
}
