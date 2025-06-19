#include <stdio.h>
#include <stdlib.h>
#include <time.h>

void cast(void *ptr) {
    if (ptr != NULL) {
        // cast the void pointer to an integer pointer
        int *int_ptr = (int *)ptr;
        printf("Value: %d\n", *int_ptr);
        // free the allocated memory
        free(int_ptr);
    }
}
// Seed rand() when the program starts
__attribute__((constructor)) void seed_random() {
    srand(time(NULL));
}