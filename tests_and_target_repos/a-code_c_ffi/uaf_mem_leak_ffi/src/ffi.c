#include <stdio.h>
#include <stdlib.h>

void free_str(char *str) {
    if (str != NULL) {
        free(str);
    }
}    
void cast_and_free_pointer(void *ptr) {

    // cast the void pointer to an integer pointer
     int *int_ptr = (int *)ptr;
    // free the allocated memory
    free(int_ptr);
     }
     
void free_int(void *ptr) {
     int *int_ptr = (int *)ptr;
     printf("Integer received in C function: %d\n", *int_ptr);
     printf("Freeing integer pointer in C...\n");
     free(int_ptr);
 }

 void free_string(char *str) {
     if (str != NULL) {
         free(str);
     }
 }    

