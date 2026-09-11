#include <stdlib.h>
#include <string.h>

char *c_alloc_string(void) {
    const char *src = "foreign";
    size_t n = strlen(src) + 1;
    char *p = (char *)malloc(n);
    if (p != NULL) memcpy(p, src, n);
    return p;
}
