#include <stdio.h>
#include <stdlib.h> 

void print_and(char* ptr) {
    printf("String: %s\n", ptr);
    free(ptr);  
}

void print(char* ptr) {
    printf("String: %s\n", ptr);
}

void cast_and_free_pointer(void *ptr) {

    // cast the void pointer to an integer pointer
     int *int_ptr = (int *)ptr;
    // free the allocated memory
    free(int_ptr);
     }