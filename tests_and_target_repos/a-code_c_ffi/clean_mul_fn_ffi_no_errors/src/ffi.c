#include <stdlib.h>
#include <string.h>

char* alloc_c_string(const char* msg) {
    char* buf = malloc(strlen(msg) + 1);
    strcpy(buf, msg);
    return buf;
}

void free_c_string(char* s) {
    free(s);
}

