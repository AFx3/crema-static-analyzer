#include <stdlib.h>

void free_first_only(int *first, int *second) {
    (void)second;
    free(first);
}