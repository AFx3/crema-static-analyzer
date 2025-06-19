#include <string.h>
#include <stdlib.h>
#include <stdio.h>

void modify_and_free_string(char* ptr) {
    if (ptr == NULL) return;

    // copy "ciao" into the provided memory (including null terminator)
    strcpy(ptr, "ciao");

    // Optional: print to check
    printf("Modified string: %s\n", ptr);

    // Free memory allocated by Rust
   free(ptr);
}

