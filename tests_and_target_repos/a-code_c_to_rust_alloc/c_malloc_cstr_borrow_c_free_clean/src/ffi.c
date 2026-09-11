#include <stdlib.h>
#include <string.h>

char *c_alloc_string(void) {
    const char *src = "hello";
    size_t n = strlen(src) + 1;
    char *p = (char *)malloc(n);
    if (p != NULL) memcpy(p, src, n);
    return p;
}

void c_free_string(char *p) {
    free(p);
}
