#include <stdio.h>
#include <stdlib.h> 

void print_and(char* ptr) {
    printf("String: %s\n", ptr);
    free(ptr);  
}

void print(char* ptr) {
    printf("String: %s\n", ptr);
}